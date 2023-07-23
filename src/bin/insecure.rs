use futures::future::BoxFuture;
use protohackers::isl::Stream;
use protohackers::{CliArgs, Parser, Server};
use std::io;
use std::io::ErrorKind;
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::TcpStream;

fn main() {
    let args = CliArgs::parse();

    let handler = Arc::new(|tcpstream| -> BoxFuture<'static, io::Result<()>> {
        Box::pin(async { handle_stream(tcpstream).await })
    });

    Server::new(args.port, args.max_connections, args.max_udp_size)
        .serve_async(handler)
        .unwrap();
}

async fn handle_stream(tcpstream: TcpStream) -> io::Result<()> {
    let mut stream = Stream::new(tcpstream).await?;

    let mut buf = Vec::new();
    loop {
        while let Some((end_of_line, _)) = buf.iter().enumerate().find(|(_, byte)| **byte == b'\n')
        {
            let line = String::from_utf8(buf.drain(..end_of_line).as_slice().to_vec())
                .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
            buf.drain(..1);
            let toy = find_toy(&line).to_owned() + "\n";
            stream.write_all(toy.as_bytes()).await?;
        }
        if stream.read_buf(&mut buf).await? == 0 {
            return Ok(());
        }
    }
}

fn find_toy(line: &str) -> &str {
    let mut from = 0;
    let mut toys = Vec::new();
    while let Some(pos) = line[from..].find(',') {
        toys.push(from..from + pos);
        from = from + pos + 1;
    }
    toys.push(from..line.len());

    toys.into_iter()
        .max_by_key(|range| {
            let pos = line[range.start..range.end].find('x').unwrap();
            u32::from_str(&line[range.start..range.start + pos]).unwrap()
        })
        .map(|range| &line[range.start..range.end])
        .unwrap()
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
