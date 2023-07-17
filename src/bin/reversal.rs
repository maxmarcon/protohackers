use protohackers::lrcp;
use protohackers::lrcp::{Socket, Stream};
use protohackers::{CliArgs, Parser, Server};
use std::io;
use std::io::ErrorKind;
use std::io::ErrorKind::Other;
use tokio::net::UdpSocket;

fn main() {
    let args = CliArgs::parse();

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_udp(&mut handler)
        .unwrap();
}

#[tokio::main]
async fn handler(udpsocket: std::net::UdpSocket, _max_size: usize) -> io::Result<()> {
    let udpsocket = UdpSocket::from_std(udpsocket).unwrap();

    let mut lrcp = Socket::new(udpsocket, 3_000, 60_000);

    loop {
        match lrcp.accept().await {
            Ok(stream) => handle_stream(stream)
                .await
                .map_err(|e| io::Error::new(Other, e))?,
            Err(error) => return Err(io::Error::new(ErrorKind::Other, error)),
        }
    }
}

async fn handle_stream(mut stream: Stream) -> lrcp::Result<()> {
    let mut buf = String::new();

    loop {
        while let Some(pos) = buf.find('\n') {
            let line = buf.drain(..pos);
            let reversed: String = line.rev().collect();
            stream.send(&reversed).await?;
        }
        buf += &stream.read().await?
    }
}
