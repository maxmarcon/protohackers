use async_stream::try_stream;
use futures::future::BoxFuture;
use futures::{pin_mut, Stream, StreamExt};
use protohackers::budgetchat::parse_message;
use protohackers::{CliArgs, Parser, Server};
use regex::Regex;
use std::sync::Arc;
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;

const UPSTREAM_SERVER: &str = "chat.protohackers.com:16963";

const TONYSBOGUSCOIN: &str = "7YWHMfk9JZe0LM0g1ZauHuiSxhI";

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
                    Some(result) =>  {
                        let msg = find_and_replace_bc(&result?);
                        server_tcp_stream_writer.write_all(format!("{msg}\n").as_bytes()).await?
                    }
                    None => break
                }
            }
            from_server = server_msg_stream.next() => {
                match from_server {
                    Some(result) => {
                        let msg = find_and_replace_bc(&result?);
                        client_tcp_stream_writer.write_all(format!("{msg}\n").as_bytes()).await?
                    }
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

fn find_and_replace_bc(msg: &str) -> String {
    let re = Regex::new(r"7[0-9a-zA-Z]{25,34}").unwrap();

    let mut new_msg = msg.to_owned();
    let matches: Vec<_> = re.find_iter(msg).collect();
    for m in matches.into_iter().rev() {
        if (m.start() == 0
            || msg
                .chars()
                .nth(m.start() - 1)
                .is_some_and(|c| c.is_ascii_whitespace()))
            && (m.end() == msg.len()
                || msg
                    .chars()
                    .nth(m.end())
                    .is_some_and(|c| c.is_ascii_whitespace()))
        {
            new_msg.replace_range(m.range(), TONYSBOGUSCOIN)
        }
    }
    new_msg
}
