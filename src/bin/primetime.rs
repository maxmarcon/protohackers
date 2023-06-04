use protohackers::{CliArgs, Parser, Server};
use serde_json::{json, value::Value, Map};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

#[derive(Debug)]
enum MessageError {
    Serde(serde_json::Error),
    UnbalancedBrackets,
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
        let read = tcpstream.read(&mut buffer[write_from..]).unwrap();
        if read == 0 {
            break;
        }

        write_from += read;

        let response = parse_message(&buffer[0..write_from])
            .map(|(json, pos)| {
                if json.is_some() {
                    write_from = 0;
                    buffer.drain(0..pos);
                    if buffer.is_empty() {
                        buffer.append(&mut vec![0_u8; READ_CHUNK]);
                    }
                } else if write_from == buffer.len() {
                    buffer.append(&mut vec![0_u8; READ_CHUNK]);
                }
                json
            })
            .and_then(|json| json.map_or(Ok(None), |json| process_message(json).map(Some)));

        println!("response = {:?}", response);
        match response {
            Ok(Some(response)) => tcpstream
                .write_all(serde_json::to_string(&response).unwrap().as_bytes())
                .unwrap(),
            Ok(None) => (),
            Err(e) => {
                tcpstream
                    .write_all(json!({ "error": format!("{e:?}") }).to_string().as_bytes())
                    .unwrap();
                break;
            }
        }
    }
}

fn parse_message(bytes: &[u8]) -> Result<(Option<Map<String, Value>>, usize), MessageError> {
    let mut open_brackets = 0;
    for (pos, byte) in bytes.iter().enumerate() {
        if pos == 0 && *byte != b'{' {
            return Err(MessageError::UnbalancedBrackets);
        }
        if *byte == b'{' {
            open_brackets += 1;
        }
        if *byte == b'}' {
            open_brackets -= 1;
        }
        if open_brackets < 0 {
            return Err(MessageError::UnbalancedBrackets);
        }

        if open_brackets <= 0 {
            let json = serde_json::from_slice(&bytes[..=pos])?;
            return match json {
                Value::Object(map) => Ok((Some(map), pos + 1)),
                _ => Err(MessageError::NotAMap),
            };
        }
    }

    Ok((None, bytes.len()))
}

fn process_message(message: Map<String, Value>) -> Result<Value, MessageError> {
    println!("processing: {:?}", message);
    match message.get("method") {
        None => return Err(MessageError::Invalid),
        Some(value) if value != "isPrime" => return Err(MessageError::Invalid),
        _ => (),
    }

    let is_prime = match message.get("number") {
        None => return Err(MessageError::Invalid),
        Some(value) if !value.is_number() => return Err(MessageError::Invalid),
        Some(number) if number.is_f64() => None,
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

        assert!(!is_prime(16));
        assert!(!is_prime(22));
        assert!(!is_prime(44));
        assert!(!is_prime(100));
    }
}
