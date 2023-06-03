use protohackers::{CliArgs, Parser, Server};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc};

fn main() {
    let args = CliArgs::parse();

    let handler: Arc<dyn Fn(TcpStream) + Send + Sync + 'static> = {
        Arc::new(move |tcpstream| {
            handle_stream(tcpstream);
        })
    };

    Server::new(args.port, args.max_connections).serve(handler);
}

const READ_SIZE: usize = 1024;

fn handle_stream(mut tcpstream: TcpStream) {
    let mut buffer = vec![0; READ_SIZE];
    let mut last = 0;

    loop {
        if tcpstream.read(&mut buffer[last..]).unwrap() == 0 {
            break;
        }

        match read_json_object(&buffer) {
            Ok((Some(json), _pos)) => {
                println!("got json {json:?}");
                buffer = vec![0; READ_SIZE];
                last = 0;
            }
            Ok((None, pos)) => {
                last = pos;
                buffer.append(&mut vec![0_u8; READ_SIZE]);
            }
            Err(()) => {
                tcpstream.write_all(b"whats up?\n").unwrap();
                break;
            }
        }
    }
}

type JsonMap = serde_json::Map<String, serde_json::Value>;

fn read_json_object(bytes: &[u8]) -> Result<(Option<JsonMap>, usize), ()> {
    let mut open_brackets = 0;
    for (pos, byte) in bytes.iter().enumerate() {
        if pos == 0 && *byte != b'{' {
            return Err(());
        }
        if *byte == b'{' {
            open_brackets += 1;
        }
        if *byte == b'}' {
            open_brackets -= 1;
        }

        if open_brackets < 0 {
            return Err(());
        }

        if open_brackets == 0 {
            match serde_json::from_slice(&bytes[..=pos]) {
                Err(_) => return Err(()),
                Ok(json) => return Ok((Some(json), pos + 1)),
            }
        }
    }

    Ok((None, bytes.len()))
}
