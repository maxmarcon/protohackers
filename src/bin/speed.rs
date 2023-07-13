use crate::ClientType::Unknown;
use async_stream::try_stream;
use core::option::Option;
use futures::future::BoxFuture;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use protohackers::speed;
use protohackers::speed::event;
use protohackers::speed::msg;
use protohackers::speed::{DecodeError, DecodedMsg};
use protohackers::{CliArgs, Parser, Server};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::io::Error;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::time;
use tokio::time::{interval, Interval};

enum ClientType {
    Camera(u16, u16, u16),     // road, mile, limit
    Dispatcher(u16, Vec<u16>), // dispatcher_id, list of roads
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

#[derive(Ord, PartialOrd, Eq, PartialEq)]
struct Reading {
    plate: String,
    road: u16,
    ts: u32,
}

impl Reading {
    pub fn new(plate: String, road: u16, ts: u32) -> Self {
        Self { plate, road, ts }
    }

    pub fn check_violation<'a>(
        reading_map: &'a ReadingMap,
        new_reading: &'a Reading,
        new_mile: u16,
        limit: u16,
    ) -> Option<(&'a Reading, &'a Reading, u16)> {
        if let Some((prev_reading, prev_mile)) = reading_map.range(..=new_reading).rev().next() {
            let speed = Reading::avg_speed((prev_reading, *prev_mile), (new_reading, new_mile));
            if speed > limit as f32 {
                return Some((prev_reading, new_reading, speed as u16));
            }
        }
        if let Some((next_reading, next_mile)) = reading_map.range(new_reading..).next() {
            let speed = Reading::avg_speed((new_reading, new_mile), (next_reading, *next_mile));
            if speed > limit as f32 {
                return Some((new_reading, next_reading, speed as u16));
            }
        }
        None
    }

    pub fn avg_speed(from: (&Reading, u16), to: (&Reading, u16)) -> f32 {
        (to.1 - from.1) as f32 / (to.0.ts - from.0.ts) as f32
    }
}

// maps a reading to the recorded miles
type ReadingMap = BTreeMap<Reading, u16>;
// maps a road to a set of dispatchers
type DispatcherMap = HashMap<u16, HashSet<u16>>;

fn main() {
    let readings = Arc::new(Mutex::new(BTreeMap::new()));

    let dispatcher_id = Arc::new(Mutex::new(1_u16));
    let dispatchers = Arc::new(Mutex::new(HashMap::new()));
    let car_last_ticket = Arc::new(Mutex::new(HashMap::new()));

    let (sender, _receiver) = broadcast::channel(100);
    let args = CliArgs::parse();

    let handler: Arc<_> = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let readings = readings.clone();
        let dispatchers = dispatchers.clone();
        let sender = sender.clone();
        let dispacher_id = dispatcher_id.clone();
        let car_last_ticket = car_last_ticket.clone();
        Box::pin(async move {
            handle_connection(
                tcpstream,
                readings,
                dispacher_id,
                dispatchers,
                car_last_ticket,
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
    mut tcpstream: TcpStream,
    readings: Arc<Mutex<ReadingMap>>,
    dispatcher_id: Arc<Mutex<u16>>,
    dispatchers: Arc<Mutex<DispatcherMap>>,
    car_last_ticket: Arc<Mutex<HashMap<String, u32>>>,
    mut sender: Sender<event::Event>,
    mut receiver: Receiver<event::Event>,
) -> io::Result<()> {
    let mut me = ClientType::Unknown;

    let (mut tcp_reader, mut tcp_writer) = tcpstream.into_split();

    let result = processing_loop(
        &mut me,
        tcp_reader,
        tcp_writer,
        readings,
        dispatcher_id,
        &dispatchers,
        car_last_ticket,
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
    car_last_ticket: Arc<Mutex<HashMap<String, u32>>>,
    mut sender: Sender<event::Event>,
    mut receiver: Receiver<event::Event>,
) -> io::Result<()> {
    let client_messages = message_stream(&mut tcp_reader);
    pin_mut!(client_messages);

    let mut heartbeat_interval: Option<Interval> = None;

    // ticket for roads with no dispatchers - waiting to be delivered when a dispatcher connects;
    let mut ticket_backlog: HashMap<u16, Vec<msg::Ticket>> = HashMap::new();

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
                            &dispatchers,
                            &mut sender,
                            me,
                            &mut heartbeat_interval).await {
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
            },
            _ = heartbeat_interval.as_mut().unwrap().tick(), if heartbeat_interval.is_some() => {
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
) -> Result<(), ProcessingError> {
    match message {
        DecodedMsg::Unknown => {
            tcpstream
                .write_all(msg::Error::new("unknown message").encode().as_slice())
                .await?;
            return Err(ProcessingError::InvalidRequest);
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
                    want_heartbeat.interval as f32 / 10 as f32,
                )))
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
                        .or_insert(HashSet::new())
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
        _ => (),
    };

    Ok(())
}
