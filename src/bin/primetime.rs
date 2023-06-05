use protohackers::{CliArgs, Parser, Server};
use serde_json::{json, value::Value, Map};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

#[derive(Debug)]
enum MessageError {
    Serde(serde_json::Error),
    NotAMap,
    Invalid,
}

impl From<serde_json::Error> for MessageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serde(error)
    }
}

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
        let bytes_read = tcpstream.read(&mut buffer[write_from..]).unwrap();
        if bytes_read == 0 {
            break;
        }

        write_from += bytes_read;

        while let Some(pos) = buffer
            .iter()
            .enumerate()
            .find(|(_, &b)| b == b'\n')
            .map(|(pos, _)| pos)
        {
            let response = parse_message(&buffer[..pos]).and_then(process_message);

            match response {
                Ok(response) => {
                    tcpstream
                        .write_all(serde_json::to_string(&response).unwrap().as_bytes())
                        .unwrap();
                    tcpstream.write_all(b"\n").unwrap();
                }
                Err(e) => {
                    tcpstream
                        .write_all(json!({ "error": format!("{e:?}") }).to_string().as_bytes())
                        .unwrap();
                    return;
                }
            }

            buffer.drain(..=pos);

            write_from -= pos + 1;
            if buffer.is_empty() {
                buffer.append(&mut vec![0_u8; READ_CHUNK]);
            }
        }

        if write_from == buffer.len() {
            buffer.append(&mut vec![0_u8; READ_CHUNK]);
        }
    }
}

fn parse_message(bytes: &[u8]) -> Result<Map<String, Value>, MessageError> {
    match serde_json::from_slice(bytes)? {
        Value::Object(map) => Ok(map),
        _ => Err(MessageError::NotAMap),
    }
}

fn process_message(message: Map<String, Value>) -> Result<Value, MessageError> {
    match message.get("method") {
        None => return Err(MessageError::Invalid),
        Some(value) if value != "isPrime" => return Err(MessageError::Invalid),
        _ => (),
    }

    let is_prime = match message.get("number") {
        None => return Err(MessageError::Invalid),
        Some(value) if !value.is_number() => return Err(MessageError::Invalid),
        Some(number) if number.is_f64() => number.as_u64(),
        Some(number) if number.is_u64() => number.as_u64(),
        _ => None,
    }
    .map_or(false, is_prime);

    Ok(json!({
        "method": "isPrime",
        "prime": is_prime
    }))
}

fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }

    let sq = (n as f64).sqrt() as u64;
    for p in 2..=sq {
        if n % p == 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::is_prime;

    #[test]
    fn test_is_prime() {
        assert!(is_prime(2));
        assert!(is_prime(3));
        assert!(is_prime(17));
        assert!(is_prime(103));
        assert!(is_prime(8297));
        assert!(is_prime(75487079));

        assert!(!is_prime(1));
        assert!(!is_prime(16));
        assert!(!is_prime(22));
        assert!(!is_prime(44));
        assert!(!is_prime(100));
    }
}
