use async_stream::try_stream;
use core::option::Option;
use futures::future::BoxFuture;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use protohackers::speed;
use protohackers::speed::event;
use protohackers::speed::event::Event;
use protohackers::speed::msg;
use protohackers::speed::{DecodeError, DecodedMsg};
use protohackers::{CliArgs, Parser, Server};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::io::Error;
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::time;
use tokio::time::Interval;

enum ClientType {
    Camera(u16, u16, u16),
    // road, mile, limit
    Dispatcher(u16, Vec<u16>),
    // dispatcher_id, list of roads
    Unknown,
}

enum ProcessingError {
    InvalidRequest,
    IOError(io::Error),
}

impl From<io::Error> for ProcessingError {
    fn from(error: Error) -> Self {
        ProcessingError::IOError(error)
    }
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Clone, Debug)]
struct Reading {
    pub ts: u32,
    pub mile: u16,
}

impl Reading {
    pub fn new(ts: u32, mile: u16) -> Self {
        Self { ts, mile }
    }

    pub fn check_violation<'a>(
        reading_map: &'a BTreeSet<Reading>,
        new_reading: &'a Reading,
        limit: u16,
    ) -> Option<(&'a Reading, &'a Reading, u16)> {
        if let Some(prev_reading) = reading_map
            .range((Unbounded, Excluded(new_reading)))
            .rev()
            .next()
        {
            let speed = Reading::avg_speed(prev_reading, new_reading);
            if speed > limit as f32 {
                return Some((prev_reading, new_reading, speed as u16));
            }
        }
        if let Some(next_reading) = reading_map.range((Excluded(new_reading), Unbounded)).next() {
            let speed = Reading::avg_speed(new_reading, next_reading);
            if speed > limit as f32 {
                return Some((new_reading, next_reading, speed as u16));
            }
        }
        None
    }

    pub fn avg_speed(from: &Reading, to: &Reading) -> f32 {
        println!("avg_speed from: {:?} to: {:?}", from, to);
        3600.0 * (to.mile - from.mile) as f32 / (to.ts - from.ts) as f32
    }
}

// maps a reading to the recorded miles
// plate, road -> set of readings
type ReadingMap = HashMap<(String, u16), BTreeSet<Reading>>;
// maps a road to a set of dispatchers
type DispatcherMap = HashMap<u16, HashSet<u16>>;
// tickets
type TicketRecord = HashMap<String, HashSet<u32>>;

fn main() {
    let readings = Arc::new(Mutex::new(HashMap::new()));

    let dispatcher_id = Arc::new(Mutex::new(1_u16));
    let dispatchers = Arc::new(Mutex::new(HashMap::new()));
    let car_tickets = Arc::new(Mutex::new(HashMap::new()));

    let (sender, _receiver) = broadcast::channel(256);
    let args = CliArgs::parse();

    let handler: Arc<_> = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let readings = readings.clone();
        let dispatchers = dispatchers.clone();
        let sender = sender.clone();
        let dispacher_id = dispatcher_id.clone();
        let car_tickets = car_tickets.clone();
        Box::pin(async move {
            handle_connection(
                tcpstream,
                readings,
                dispacher_id,
                dispatchers,
                car_tickets,
                sender.clone(),
                sender.subscribe(),
            )
            .await
        })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_connection(
    tcpstream: TcpStream,
    readings: Arc<Mutex<ReadingMap>>,
    dispatcher_id: Arc<Mutex<u16>>,
    dispatchers: Arc<Mutex<DispatcherMap>>,
    car_tickets: Arc<Mutex<HashMap<String, HashSet<u32>>>>,
    sender: Sender<event::Event>,
    receiver: Receiver<event::Event>,
) -> io::Result<()> {
    let mut me = ClientType::Unknown;

    let (tcp_reader, tcp_writer) = tcpstream.into_split();

    let result = processing_loop(
        &mut me,
        tcp_reader,
        tcp_writer,
        readings,
        dispatcher_id,
        &dispatchers,
        car_tickets,
        sender,
        receiver,
    )
    .await;

    if let ClientType::Dispatcher(dispatcher_id, roads) = me {
        // unregister dispatcher
        let mut dispatchers = dispatchers.lock().unwrap();
        for road in roads {
            dispatchers.entry(road).and_modify(|dispatchers| {
                dispatchers.remove(&dispatcher_id);
            });
        }
    }

    result
}

async fn processing_loop(
    me: &mut ClientType,
    mut tcp_reader: OwnedReadHalf,
    mut tcp_writer: OwnedWriteHalf,
    readings: Arc<Mutex<ReadingMap>>,
    dispatcher_id: Arc<Mutex<u16>>,
    dispatchers: &Arc<Mutex<DispatcherMap>>,
    car_tickets: Arc<Mutex<HashMap<String, HashSet<u32>>>>,
    mut sender: Sender<event::Event>,
    mut receiver: Receiver<event::Event>,
) -> io::Result<()> {
    let client_messages = message_stream(&mut tcp_reader);
    pin_mut!(client_messages);

    let mut heartbeat_interval: Option<Interval> = None;

    // ticket for roads with no dispatchers - waiting to be delivered when a dispatcher connects;
    let mut ticket_backlog: Vec<msg::Ticket> = Vec::new();

    loop {
        tokio::select! {
            result = client_messages.next() => {
                if result.is_none() {
                    break;
                }
                match result.unwrap() {
                    Err(DecodeError::IOError(io_error)) => {
                        return Err(io_error);
                    }
                    Ok(decoded_msg) => if let Err(error) =
                        process_message(
                            decoded_msg,
                            &mut tcp_writer,
                            &readings,&dispatcher_id,
                            dispatchers,
                            &mut sender,
                            me,
                            &mut heartbeat_interval,
                            &mut ticket_backlog,
                            &car_tickets).await {
                        match error {
                            ProcessingError::InvalidRequest => {
                                break;
                            },
                            ProcessingError::IOError(io_error) => {
                                return Err(io_error);
                            }
                        }
                    }
                    _ => (),
                }
            },
            event = receiver.recv() => {
                let event = event.unwrap();
                process_event(event, me, &mut tcp_writer, &mut sender, &mut ticket_backlog, &car_tickets).await?;
            },
            _ = async { heartbeat_interval.as_mut().unwrap().tick().await }, if heartbeat_interval.is_some() => {
                tcp_writer.write_all(msg::Heartbeat::encode().as_slice()).await?
            }
        }
    }
    Ok(())
}

fn message_stream(
    tcpstream: &mut OwnedReadHalf,
) -> impl Stream<Item = Result<DecodedMsg, DecodeError>> + '_ {
    let mut buffer = Vec::new();
    try_stream! {
        loop {
            let result = speed::decode_msg(&buffer);
            match result {
                Ok(msg) => {
                    buffer.drain(..msg.len());
                    yield msg;
                },
                Err(speed::DecodeError::TooShort) => {
                    let read_bytes = tcpstream.read_buf(&mut buffer).await?;
                    if read_bytes == 0 {
                        break;
                    }
                },
                error => { error?; }
            }
        }
    }
}

async fn process_message(
    message: DecodedMsg,
    tcpstream: &mut OwnedWriteHalf,
    readings: &Arc<Mutex<ReadingMap>>,
    dispatcher_id: &Arc<Mutex<u16>>,
    dispatchers: &Arc<Mutex<DispatcherMap>>,
    sender: &mut Sender<event::Event>,
    me: &mut ClientType,
    heartbeat_interval: &mut Option<Interval>,
    ticket_backlog: &mut Vec<msg::Ticket>,
    ticket_record: &Arc<Mutex<TicketRecord>>,
) -> Result<(), ProcessingError> {
    match message {
        DecodedMsg::Unknown => {
            tcpstream
                .write_all(msg::Error::new("unknown message").encode().as_slice())
                .await?;
            return Err(ProcessingError::InvalidRequest);
        }
        DecodedMsg::Plate(plate) => {
            let (road, mile, limit) = match *me {
                ClientType::Camera(road, mile, limit) => (road, mile, limit),
                _ => {
                    tcpstream
                        .write_all(msg::Error::new("unknown message").encode().as_slice())
                        .await?;
                    return Err(ProcessingError::InvalidRequest);
                }
            };

            let new_reading = Reading::new(plate.ts, mile);
            let mut all_readings = readings.lock().unwrap();
            let car_readings = all_readings.entry((plate.plate.clone(), road)).or_default();
            car_readings.insert(new_reading.clone());

            if let Some((from, to, speed)) =
                Reading::check_violation(car_readings, &new_reading, limit)
            {
                let ticket = msg::Ticket::new(
                    &plate.plate,
                    road,
                    from.mile,
                    from.ts,
                    to.mile,
                    to.ts,
                    speed * 100,
                );

                let mut ticket_record = ticket_record.lock().unwrap();
                let car_ticket_days = ticket_record.entry(plate.plate.clone()).or_default();

                if !ticket.overlaps(car_ticket_days) {
                    // car wasn't still ticketed on any of the ticket's day - let's issue the ticket
                    // find a dispatcher for the ticket
                    let dispatchers = dispatchers.lock().unwrap();
                    if let Some(dispatcher_id) = dispatchers
                        .get(&road)
                        .and_then(|dispatchers| dispatchers.iter().next())
                    {
                        // a dispatcher is online - send ticket
                        ticket.record(car_ticket_days);
                        sender.send(Event::Ticket(ticket, *dispatcher_id)).unwrap();
                    } else {
                        // no dispatcher online - store ticket
                        ticket_backlog.push(ticket);
                    }
                }
            }
        }
        DecodedMsg::WantHeartbeat(want_heartbeat) => {
            if heartbeat_interval.is_some() {
                tcpstream
                    .write_all(msg::Error::new("heartbeat already set").encode().as_slice())
                    .await?;
                return Err(ProcessingError::InvalidRequest);
            }
            if want_heartbeat.interval > 0 {
                *heartbeat_interval = Some(time::interval(Duration::from_secs_f32(
                    want_heartbeat.interval as f32 / 10_f32,
                )));
            }
        }
        DecodedMsg::IAmCamera(iam_camera) => match me {
            ClientType::Unknown => {
                *me = ClientType::Camera(iam_camera.road, iam_camera.mile, iam_camera.limit)
            }
            _ => {
                tcpstream
                    .write_all(
                        msg::Error::new("client already identified itself")
                            .encode()
                            .as_slice(),
                    )
                    .await?
            }
        },
        DecodedMsg::IAmDispatcher(iam_dispatcher) => match me {
            ClientType::Unknown => {
                let mut this_dispatcher = dispatcher_id.lock().unwrap();
                *me = ClientType::Dispatcher(*this_dispatcher, iam_dispatcher.roads.clone());
                let mut dispatchers = dispatchers.lock().unwrap();
                for &road in iam_dispatcher.roads.iter() {
                    dispatchers
                        .entry(road)
                        .or_default()
                        .insert(*this_dispatcher);
                }
                sender
                    .send(event::Event::NewDispatcher(
                        *this_dispatcher,
                        iam_dispatcher.roads.clone(),
                    ))
                    .unwrap();
                *this_dispatcher += 1;
            }
            _ => {
                tcpstream
                    .write_all(
                        msg::Error::new("client already identified itself")
                            .encode()
                            .as_slice(),
                    )
                    .await?
            }
        },
    };

    Ok(())
}

async fn process_event(
    event: Event,
    me: &ClientType,
    tcpstream: &mut OwnedWriteHalf,
    sender: &mut Sender<Event>,
    ticket_backlog: &mut Vec<msg::Ticket>,
    ticket_record: &Arc<Mutex<TicketRecord>>,
) -> io::Result<()> {
    match event {
        Event::Ticket(ticket, target_dispatcher_id) => match *me {
            ClientType::Dispatcher(my_dispatcher_id, _)
                if my_dispatcher_id == target_dispatcher_id =>
            {
                tcpstream.write_all(ticket.encode().as_slice()).await?
            }
            _ => (),
        },
        Event::NewDispatcher(dispatcher_id, roads) => match *me {
            ClientType::Camera(my_road, _, _) if roads.contains(&my_road) => {
                ticket_backlog.drain(..).for_each(|ticket| {
                    let mut ticket_record = ticket_record.lock().unwrap();
                    let car_ticket_days = ticket_record.entry(ticket.plate.clone()).or_default();
                    if !ticket.overlaps(car_ticket_days) {
                        ticket.record(car_ticket_days);
                        sender.send(Event::Ticket(ticket, dispatcher_id)).unwrap();
                    }
                });
            }
            _ => (),
        },
    }

    Ok(())
}
