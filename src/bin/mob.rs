use futures::future::BoxFuture;
use protohackers::{CliArgs, Parser, Server};
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::io;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

fn main() {
    let args = CliArgs::parse();

    let handler: Arc<_> = Arc::new(|tcpstream| -> BoxFuture<'static, io::Result<()>> {
        Box::pin(async { handle(tcpstream).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle(mut tcpstream: TcpStream) -> io::Result<()> {
    let mut buffer = Vec::new();
    loop {
        let read_bytes = tcpstream.read_buf(&mut buffer).await?;
        if read_bytes == 0 {
            return Ok(());
        }

        while let Some(msg) = parse_message(&mut buffer)? {}
    }
}

fn parse_message(buffer: &mut Vec<u8>) -> io::Result<Option<String>> {
    if let Some(pos) = buffer
        .iter()
        .enumerate()
        .find(|(_pos, c)| **c == b'\n')
        .map(|(pos, _)| pos)
    {
        return String::from_utf8(buffer.drain(..=pos).collect())
            .map(|msg| Some(msg.trim().to_owned()))
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e));
    }
    Ok(None)
}
