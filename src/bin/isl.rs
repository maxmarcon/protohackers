use futures::future::BoxFuture;
use protohackers::isl::Stream;
use protohackers::{CliArgs, Parser, Server};
use std::io;
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::TcpStream;

fn main() {
    let args = CliArgs::parse();

    let handler = Arc::new(move |tcpstream| -> BoxFuture<'static, io::Result<()>> {
        Box::pin(async { handle_stream(tcpstream).await.unwrap(); Ok(()) })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_stream(tcpstream: TcpStream) -> io::Result<()> {
    let mut stream = Stream::new(tcpstream).await?;

    loop {
        let line = stream.read_line().await?;
        stream.write_line(&find_toy(&line)).await?
    }
}

fn find_toy(line: &str) -> String {
    let mut from = 0;
    let mut toys = Vec::new();
    while let Some(pos) = line[from..].find(',') {
        toys.push(&line[from..from + pos]);
        from = from + pos + 1;
    }
    toys.push(&line[from..]);

    toys.iter()
        .max_by_key(|toy| {
            let pos = toy.find('x').unwrap();
            u32::from_str(&toy[..pos]).unwrap()
        })
        .unwrap()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::find_toy;

    #[test]
    fn test_find_toy() {
        assert_eq!(
            find_toy("10x toy car,15x dog on a string,4x inflatable motorcycle"),
            "15x dog on a string"
        );
        assert_eq!(find_toy("15x dog on a string"), "15x dog on a string");
    }
}
