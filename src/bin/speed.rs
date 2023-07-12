use crate::ClientType::Unknown;
use async_stream::try_stream;
use futures::future::BoxFuture;
use futures::StreamExt;
use futures::{pin_mut, Stream};
use protohackers::speed;
use protohackers::speed::msg::Ticket;
use protohackers::speed::{DecodeError, DecodedMsg};
use protohackers::{CliArgs, Parser, Server};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::io::Read;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};

enum ClientType {
    Camera,
    Dispatcher,
    Unknown,
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

    let dispatchers = Arc::new(Mutex::new(HashMap::new()));

    let (sender, _receiver) = broadcast::channel(100);
    let args = CliArgs::parse();

    let handler: Arc<_> = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        let readings = readings.clone();
        let dispatchers = dispatchers.clone();
        let sender = sender.clone();
        Box::pin(async move {
            handle_connection(
                tcpstream,
                readings,
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
    dispatchers: Arc<Mutex<DispatcherMap>>,
    mut sender: Sender<Ticket>,
    mut receiver: Receiver<Ticket>,
) -> io::Result<()> {
    let iam = ClientType::Unknown;

    let client_messages = message_stream(&mut tcpstream);
    pin_mut!(client_messages);

    tokio::select! {
        result = client_messages.next() => {
        },
        event = receiver.recv() => {
        }
    }

    Ok(())
}

fn message_stream(
    tcpstream: &mut TcpStream,
) -> impl Stream<Item = Result<DecodedMsg, DecodeError>> + '_ {
    let mut buffer = Vec::new();
    try_stream! {
        loop {
            let result = speed::decode_msg(&buffer);
            match result {
                Ok(msg) => { yield msg; }
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
