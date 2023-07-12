use futures::future::BoxFuture;
use protohackers::{CliArgs, Parser, Server};
use serde_json::map::OccupiedEntry;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::io::Read;
use std::sync::Arc;
use tokio::net::TcpStream;

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

fn main() {
    let readings: ReadingMap = BTreeMap::new();

    let args = CliArgs::parse();

    let handler: Arc<_> = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        Box::pin(async { handle_connection(tcpstream) })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

fn handle_connection(tcpstream: TcpStream) -> io::Result<()> {
    Ok(())
}
