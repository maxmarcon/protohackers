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
use std::cmp::{max, min};
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
use tokio::sync::broadcast::Sender;
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

    pub fn make_tickets<'a>(
        reading_map: &'a BTreeSet<Reading>,
        new_reading: &'a Reading,
        limit: u16,
        road: u16,
        plate: &str,
    ) -> Vec<msg::Ticket> {
        let earlier_reading = reading_map
            .range((Unbounded, Excluded(new_reading)))
            .next_back();
        let later_reading = reading_map.range((Excluded(new_reading), Unbounded)).next();

        [earlier_reading, later_reading]
            .iter()
            .flat_map(|other_reading| {
                other_reading.map(|other_reading| {
                    let speed = Reading::avg_speed(new_reading, other_reading);
                    if speed > limit as f32 {
                        let reading1 = min(new_reading, other_reading);
                        let reading2 = max(new_reading, other_reading);

                        Some(msg::Ticket::new(
                            plate,
                            road,
                            reading1.mile,
                            reading1.ts,
                            reading2.mile,
                            reading2.ts,
                            (speed * 100.0) as u16,
                        ))
                    } else {
                        None
                    }
                })
            })
            .flatten()
            .collect()
    }

    pub fn avg_speed(reading_1: &Reading, reading_2: &Reading) -> f32 {
        let max_mile = max(reading_1.mile, reading_2.mile);
        let min_mile = min(reading_1.mile, reading_2.mile);
        let max_ts = max(reading_1.ts, reading_2.ts);
        let min_ts = min(reading_1.ts, reading_2.ts);

        3600.0 * (max_mile - min_mile) as f32 / (max_ts - min_ts) as f32
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

    let dispatcher_id = Arc::new(Mutex::new(0_u16));
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
        Box::pin(async {
            handle_connection(
                tcpstream,
                readings,
                dispacher_id,
                dispatchers,
                car_tickets,
                sender,
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
    mut sender: Sender<Event>,
) -> io::Result<()> {
    let client_messages = message_stream(&mut tcp_reader);
    pin_mut!(client_messages);

    let mut heartbeat_interval: Option<Interval> = None;

    // ticket for roads with no dispatchers - waiting to be delivered when a dispatcher connects;
    let mut ticket_backlog: Vec<msg::Ticket> = Vec::new();
    let mut receiver = sender.subscribe();

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
                            &readings,
                            &dispatcher_id,
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
                    buffer.drain(..msg.size());
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
            println!("CAR {}\nLIMIT {}\nREADINGS:", plate.plate, limit);
            print_readings(car_readings);

            for ticket in
                Reading::make_tickets(car_readings, &new_reading, limit, road, &plate.plate)
            {
                println!("TICKET!!! -- {:?} -- ON DAYS: {:?}", ticket, ticket.days());

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
                        println!("TICKET SENT");
                    } else {
                        // no dispatcher online - store ticket
                        ticket_backlog.push(ticket);
                        println!("TICKET SAVED");
                    }
                } else {
                    println!(
                        "TICKET NOT SENT BECAUSE CARS TICKETED ON: {:?}",
                        car_ticket_days
                    );
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
                let my_dispatcher_id = {
                    let mut this_dispatcher = dispatcher_id.lock().unwrap();
                    *this_dispatcher += 1;
                    *this_dispatcher
                };
                *me = ClientType::Dispatcher(my_dispatcher_id, iam_dispatcher.roads.clone());
                let mut dispatchers = dispatchers.lock().unwrap();
                for &road in iam_dispatcher.roads.iter() {
                    dispatchers
                        .entry(road)
                        .or_default()
                        .insert(my_dispatcher_id);
                }
                sender
                    .send(event::Event::NewDispatcher(
                        my_dispatcher_id,
                        iam_dispatcher.roads.clone(),
                    ))
                    .unwrap();
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

fn print_readings(readings: &BTreeSet<Reading>) {
    let mut iter = readings.iter().peekable();
    while let Some(r1) = iter.next() {
        print!("{:?}", r1);
        if let Some(&r2) = iter.peek() {
            print!(" --> {}", Reading::avg_speed(r1, r2))
        }
        println!();
    }
}
