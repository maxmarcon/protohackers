use async_stream::try_stream;
use futures::future::BoxFuture;
use futures::{pin_mut, Stream, StreamExt};
use protohackers::{CliArgs, Parser, Server};
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;

const UPSTREAM_SERVER: &str = "chat.protohackers.com:16963";

fn main() {
    let args = CliArgs::parse();

    let handler: Arc<_> = Arc::new(|tcpstream| -> BoxFuture<'static, io::Result<()>> {
        Box::pin(async { handle(tcpstream).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle(client_tcp_stream: TcpStream) -> io::Result<()> {
    let (mut client_tcp_stream_reader, mut client_tcp_stream_writer) =
        client_tcp_stream.into_split();

    let client_msg_stream = read_message(&mut client_tcp_stream_reader);
    pin_mut!(client_msg_stream);

    let server_tcp_stream = TcpStream::connect(UPSTREAM_SERVER).await?;
    let (mut server_tcp_stream_reader, mut server_tcp_stream_writer) =
        server_tcp_stream.into_split();
    let server_msg_stream = read_message(&mut server_tcp_stream_reader);
    pin_mut!(server_msg_stream);

    loop {
        tokio::select! {
            from_client = client_msg_stream.next() => {
                match from_client {
                    Some(result) => server_tcp_stream_writer.write_all(result?.as_bytes()).await?,
                    None => break
                }
            }
            from_server = server_msg_stream.next() => {
                match from_server {
                    Some(result) => client_tcp_stream_writer.write_all(result?.as_bytes()).await?,
                    None => break
                }
            }
        }
    }
    Ok(())
}

fn read_message(tcpstream: &mut OwnedReadHalf) -> impl Stream<Item = io::Result<String>> + '_ {
    let mut buffer = Vec::new();
    try_stream! {
        loop {
            while let Some(msg) = parse_message(&mut buffer)? {
                yield msg;
            }
            let read_bytes = tcpstream.read_buf(&mut buffer).await?;
            if read_bytes == 0 {
                break;
            }
        }
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
            .map(Some)
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e));
    }
    Ok(None)
}
