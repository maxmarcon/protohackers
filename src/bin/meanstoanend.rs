use protohackers::{CliArgs, Parser, Server};
use std::collections::BTreeMap;
use std::io;
use std::io::{Read, Write};
use std::net::TcpStream;

struct Insert {
    ts: i32,
    price: i32,
}

impl Insert {
    fn new(buf: &[u8]) -> Self {
        Self {
            ts: i32::from_be_bytes(buf[0..4].try_into().unwrap()),
            price: i32::from_be_bytes(buf[4..].try_into().unwrap()),
        }
    }
}

struct Query {
    mints: i32,
    maxts: i32,
}

impl Query {
    fn new(buf: &[u8]) -> Self {
        Self {
            mints: i32::from_be_bytes(buf[0..4].try_into().unwrap()),
            maxts: i32::from_be_bytes(buf[4..].try_into().unwrap()),
        }
    }
}

enum Message {
    I(Insert),
    Q(Query),
}

fn main() {
    let args = CliArgs::parse();

    let handler = handle_stream;

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve(handler.into())
        .unwrap();
}

const BUFFER_SIZE: usize = 9 * 100;

fn handle_stream(mut tcpstream: TcpStream) -> io::Result<()> {
    let mut buf = Vec::from([0; BUFFER_SIZE]);
    let mut write_from = 0;

    let mut prices = BTreeMap::<i32, i32>::new();

    loop {
        let read = tcpstream.read(&mut buf[write_from..]).unwrap();
        if read == 0 {
            break;
        }
        write_from += read;

        while write_from >= 9 {
            let message = match parse_message(&buf[..9]) {
                Ok(message) => message,
                Err(()) => {
                    tcpstream.write_all(&[0]).unwrap();
                    break;
                }
            };

            match message {
                Message::I(Insert { ts, price }) => {
                    prices.insert(ts, price);
                }
                Message::Q(Query { mints, maxts }) => {
                    let mean = get_mean_price(&prices, mints, maxts);
                    tcpstream.write_all(&mean.to_be_bytes()).unwrap();
                }
            }

            buf.drain(..9);
            write_from -= 9;
        }

        if buf.is_empty() {
            buf = Vec::from([0; BUFFER_SIZE]);
        }
    }
    Ok(())
}

fn parse_message(buf: &[u8]) -> Result<Message, ()> {
    match buf[0] {
        b'I' => Ok(Message::I(Insert::new(&buf[1..9]))),
        b'Q' => Ok(Message::Q(Query::new(&buf[1..9]))),
        _ => Err(()),
    }
}

fn get_mean_price(prices: &BTreeMap<i32, i32>, mints: i32, maxts: i32) -> i32 {
    if mints > maxts {
        return 0;
    }

    let mut cnt = 0;
    let mut sum: i64 = 0;
    for (_k, v) in prices.range(mints..=maxts) {
        sum += *v as i64;
        cnt += 1;
    }

    if cnt == 0 {
        0
    } else {
        (sum / cnt) as i32
    }
}
