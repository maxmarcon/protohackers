use protohackers::lrcp;
use protohackers::lrcp::{Socket, Stream};
use protohackers::{CliArgs, Parser, Server};
use std::io;
use std::io::ErrorKind;
use std::ops::{Add, Sub};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::{sleep_until, Instant};

fn main() {
    let args = CliArgs::parse();

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_udp(&mut handler)
        .unwrap();
}

#[tokio::main]
async fn handler(udpsocket: std::net::UdpSocket, _max_size: usize) -> io::Result<()> {
    udpsocket.set_nonblocking(true)?;
    let udpsocket = UdpSocket::from_std(udpsocket).unwrap();

    let mut lrcp = Socket::new(udpsocket, 3_000, 60_000);

    loop {
        match lrcp.accept().await {
            Ok(stream) => {
                tokio::spawn(async move { handle_stream(stream).await });
            }
            Err(error) => return Err(io::Error::new(ErrorKind::Other, error)),
        }
    }
}

async fn handle_stream(mut stream: Stream) -> lrcp::Result<()> {
    let mut buf = String::new();

    loop {
        while let Some(pos) = buf.find('\n') {
            let line = buf.drain(..pos).collect::<String>();
            buf.drain(..1);
            let reversed: String = line.chars().rev().collect::<String>() + "\n";
            stream.send(&reversed).await?;
        }
        buf += &stream.read().await?;
    }
}
