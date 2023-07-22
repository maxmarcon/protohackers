use protohackers::lrcp;
use protohackers::lrcp::{Socket, Stream};
use protohackers::{CliArgs, Parser, Server};
use std::io;
use tokio::net::UdpSocket;

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
        let stream = lrcp.accept().await?;
        tokio::spawn(async move { handle_stream(stream).await.unwrap() });
    }
}

async fn handle_stream(mut stream: Stream) -> lrcp::Result<()> {
    let mut buf = String::new();

    loop {
        while let Some(pos) = buf.find('\n') {
            let mut line: String = buf.drain(..=pos).collect();
            line.pop();
            let mut reversed: String = line.chars().rev().collect();
            reversed.push('\n');
            stream.send(&reversed).await?;
        }
        buf += &stream.read().await?;
    }
}
