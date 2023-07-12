use crate::ClientType::Unknown;
use async_stream::try_stream;
use futures::future::BoxFuture;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use protohackers::speed;
use protohackers::speed::msg::Ticket;
use protohackers::speed::{msg, DecodeError, DecodedMsg};
use protohackers::{CliArgs, Parser, Server};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::io::{Error, Read};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};

enum ClientType {
    Camera((u16, u16, u16)),
    Dispatcher(u16),
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

type ReadingMap = BTreeMap<Reading, u16>;
type DispatcherMap = HashMap<u16, Vec<u16>>;

fn main() {
    let readings = Arc::new(Mutex::new(BTreeMap::new()));

    let dispatcher_id = Arc::new(Mutex::new(1_u16));
    let dispatchers = Arc::new(Mutex::new(HashMap::new()));

    let (sender, _receiver) = broadcast::channel(100);
    let args = CliArgs::parse();

    let handler: Arc<_> = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let readings = readings.clone();
        let dispatchers = dispatchers.clone();
        let sender = sender.clone();
        let dispacher_id = dispatcher_id.clone();
        Box::pin(async move {
            handle_connection(
                tcpstream,
                readings,
                dispacher_id,
                dispatchers,
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
    mut sender: Sender<Ticket>,
    mut receiver: Receiver<Ticket>,
) -> io::Result<()> {
    let mut me = ClientType::Unknown;

    let (mut tcp_reader, mut tcp_writer) = tcpstream.into_split();
    let client_messages = message_stream(&mut tcp_reader);
    pin_mut!(client_messages);

    loop {
        tokio::select! {
            result = client_messages.next() => {
                if result.is_none() {
                    break;
                }
                match result.unwrap() {
                    Err(DecodeError::TooShort) => (),
                    Err(DecodeError::Unknown) => {
                        tcp_writer.write_all(msg::Error::new("unknown message").encode().as_slice()).await?;
                    },
                    Err(DecodeError::IOError(io_error)) => return Err(io_error),
                    Ok(decoded_msg) => if let Err(error) = process_message(decoded_msg, &mut tcp_writer, &readings,&dispatcher_id, &dispatchers, &mut sender, &mut me).await {
                        match error {
                            ProcessingError::InvalidRequest => break,
                            ProcessingError::IOError(io_error) => return Err(io_error)
                        }
                    }
                }
            },
            event = receiver.recv() => {
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
    sender: &mut Sender<Ticket>,
    me: &mut ClientType,
) -> Result<(), ProcessingError> {
    match message {
        DecodedMsg::IAmCamera(iam_camera) => match me {
            ClientType::Unknown => {
                *me = ClientType::Camera((iam_camera.road, iam_camera.mile, iam_camera.limit))
            }
            _ => {
                tcpstream
                    .write_all(msg::Error::new("invalid request").encode().as_slice())
                    .await?
            }
        },
        DecodedMsg::IAmDispatcher(iam_dispatcher) => match me {
            ClientType::Unknown => {
                let mut this_dispatcher = dispatcher_id.lock().unwrap();
                *me = ClientType::Dispatcher(*this_dispatcher);
                *this_dispatcher += 1;
                let mut dispatchers = dispatchers.lock().unwrap();
                for road in iam_dispatcher.roads {
                    dispatchers
                        .entry(road)
                        .and_modify(|dispatchers_vec| dispatchers_vec.push(*this_dispatcher))
                        .or_insert(Vec::from([*this_dispatcher]));
                }
            }
            _ => {
                tcpstream
                    .write_all(msg::Error::new("invalid request").encode().as_slice())
                    .await?
            }
        },
        _ => (),
    };

    Ok(())
}
