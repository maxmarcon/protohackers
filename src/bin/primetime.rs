use protohackers::{CliArgs, Parser, Server};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

fn main() {
    let args = CliArgs::parse();

    let handler: Arc<dyn Fn(TcpStream) + Send + Sync + 'static> = {
        Arc::new(move |tcpstream| {
            handle_stream(tcpstream);
        })
    };

    Server::new(args.port, args.max_connections).serve(handler);
}

const READ_CHUNK: usize = 256;

fn handle_stream(mut tcpstream: TcpStream) {
    let mut buffer = vec![0; READ_CHUNK];
    let mut write_from = 0;

    loop {
        let read = tcpstream.read(&mut buffer[write_from..]).unwrap();
        if read == 0 {
            break;
        }
        
        write_from += read;
        match read_json_object(&buffer[0..write_from]) {
            Ok((Some(json), pos)) => {
                write_from = 0;
                buffer.drain(0..pos);
                if buffer.is_empty() {
                    buffer.append(&mut vec![0_u8; READ_CHUNK]);
                }
            }
            Ok((None, _pos)) => {
                if write_from == buffer.len() {
                    buffer.append(&mut vec![0_u8; READ_CHUNK]);
                }
            }
            Err(e) => {
                tcpstream.write_all(format!("{e:?}").as_bytes()).unwrap();
                break;
            }
        }
    }
}

type JsonMap = serde_json::Map<String, serde_json::Value>;

#[derive(Debug)]
enum DecodeError {
    Serde(serde_json::Error),
    UnbalancedBrackets,
}

impl From<serde_json::Error> for DecodeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serde(error)
    }
}

fn read_json_object(bytes: &[u8]) -> Result<(Option<JsonMap>, usize), DecodeError> {
    let mut open_brackets = 0;
    for (pos, byte) in bytes.iter().enumerate() {
        if pos == 0 && *byte != b'{' {
            return Err(DecodeError::UnbalancedBrackets);
        }
        if *byte == b'{' {
            open_brackets += 1;
        }
        if *byte == b'}' {
            open_brackets -= 1;
        }
        if open_brackets < 0 {
            return Err(DecodeError::UnbalancedBrackets);
        }

        if open_brackets <= 0 {
            let json = serde_json::from_slice(&bytes[..=pos])?;
            return Ok((Some(json), pos + 1));
        }
    }

    Ok((None, bytes.len()))
}
