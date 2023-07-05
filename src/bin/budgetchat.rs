use futures::future::BoxFuture;
use protohackers::{CliArgs, Parser, Server};
use std::io::ErrorKind::{InvalidData, InvalidInput};
use tokio::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn main() {
    let args = CliArgs::parse();

    Server::new(args.port, args.max_connections)
        .serve_async(handle_stream)
        .unwrap();
}

fn handle_stream(mut tcp_stream: TcpStream) -> BoxFuture<'static, io::Result<()>> {
    Box::pin(async move {
        let mut buffer = Vec::new();
        let mut user_name = None;
        tcp_stream.write_all(b"Hello, what's your name?").await?;

        loop {
            match recv_and_process(&mut tcp_stream, &mut buffer, &mut user_name).await {
                error @ Err(_) => {
                    tcp_stream
                        .write_all(error.as_ref().unwrap_err().to_string().as_bytes())
                        .await?;
                    return error;
                }
                Ok(_) => (),
            }
        }
    })
}

async fn recv_and_process(
    tcp_stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    user_name: &mut Option<String>,
) -> io::Result<()> {
    let read_bytes = tcp_stream.read_buf(buffer).await?;
    if read_bytes == 0 {
        return Ok(());
    }
    if let Some(msg) = parse_message(buffer)? {
        if user_name.is_none() {
            *user_name = Some(validate_name(msg)?);
        } else {
            tcp_stream.write_all(msg.as_bytes()).await?;
        }
    }
    Ok(())
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
            .map_err(|e| io::Error::new(InvalidData, e));
    }
    Ok(None)
}

fn validate_name(name: String) -> io::Result<String> {
    if name.chars().any(|c| !c.is_alphanumeric()) {
        return Err(io::Error::new(InvalidInput, "invalid name"));
    }
    Ok(name)
}
