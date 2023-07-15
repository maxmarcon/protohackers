use clap::builder::TypedValueParser;
use regex::{Match, Regex};
use std::io;
use std::io::Error;
use std::num::ParseIntError;
use std::str::FromStr;
use std::string::FromUtf8Error;

#[derive(Debug)]
pub enum DecodeError {
    Unknown,
    Invalid,
    IOError(Error),
    ParseUtf8Error(FromUtf8Error),
    ParseIntError(ParseIntError),
}

impl From<Error> for DecodeError {
    fn from(error: Error) -> Self {
        DecodeError::IOError(error)
    }
}

impl From<FromUtf8Error> for DecodeError {
    fn from(error: FromUtf8Error) -> Self {
        DecodeError::ParseUtf8Error(error)
    }
}

impl From<ParseIntError> for DecodeError {
    fn from(error: ParseIntError) -> Self {
        DecodeError::ParseIntError(error)
    }
}

#[derive(Debug, PartialEq)]
pub enum Decoded {
    Connect(Connect),
    Close(Close),
    Ack(Ack),
}

#[derive(Debug, PartialEq)]
struct Connect {
    session: i32,
}

impl Connect {
    pub fn new(session: i32) -> Self {
        Self { session }
    }
}

#[derive(Debug, PartialEq)]
struct Close {
    session: i32,
}

impl Close {
    pub fn new(session: i32) -> Self {
        Self { session }
    }

    pub fn encode(&self) -> String {
        format!("/close/{}/", self.session)
    }
}

#[derive(Debug, PartialEq)]
struct Ack {
    session: i32,
    length: i32,
}

impl Ack {
    pub fn new(session: i32, length: i32) -> Self {
        Self { session, length }
    }
    pub fn encode(&self) -> String {
        format!("/ack/{}/{}/", self.session, self.length)
    }
}

pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    let mut str = String::from_utf8(bytes.into())?;
    if !str.starts_with('/') || !str.ends_with('/') {
        return Err(DecodeError::Invalid);
    }

    let mut str = str.trim_matches('/');
    let mut pieces = Vec::new();
    while let Some(sep) = str.find('/') {
        if sep == 0 || &str[sep - 1..sep] != "\\" {
            pieces.push(&str[..sep]);
            str = &str[sep + 1..];
        }
    }
    pieces.push(str);

    if pieces.len() < 2 {
        return Err(DecodeError::Invalid);
    }

    match pieces[..] {
        ["connect", session] => Ok(Decoded::Connect(Connect::new(i32::from_str(session)?))),
        ["close", session] => Ok(Decoded::Close(Close::new(i32::from_str(session)?))),
        ["ack", session, length] => Ok(Decoded::Ack(Ack::new(
            i32::from_str(session)?,
            i32::from_str(length)?,
        ))),
        _ => Err(DecodeError::Invalid),
    }
}

#[cfg(test)]
mod tests {
    use super::decode;
    use super::DecodeError;
    use super::Decoded;
    use crate::lrcp::msg::{Ack, Close, Connect};

    #[test]
    fn decode_error() {
        let decoded = decode(b"/connect/");
        assert!(decoded.is_err());
        let decoded = decoded.unwrap_err();
        assert!(matches!(decoded, DecodeError::Invalid));
    }

    #[test]
    fn decode_connect() {
        let decoded = decode(b"/connect/1234567/");
        assert!(decoded.is_ok());
        let decoded = decoded.unwrap();
        assert_eq!(decoded, Decoded::Connect(Connect::new(1234567)));
    }

    #[test]
    fn decode_close() {
        let decoded = decode(b"/close/1234567/");
        assert!(decoded.is_ok());
        let decoded = decoded.unwrap();
        assert_eq!(decoded, Decoded::Close(Close::new(1234567)));
    }

    #[test]
    fn encode_close() {
        assert_eq!(Close::new(1234567).encode().as_bytes(), b"/close/1234567/");
    }

    #[test]
    fn decode_ack() {
        let decoded = decode(b"/ack/1234567/1024/");
        assert!(decoded.is_ok());
        let decoded = decoded.unwrap();
        assert_eq!(decoded, Decoded::Ack(Ack::new(1234567, 1024)));
    }

    #[test]
    fn encode_ack() {
        assert_eq!(
            Ack::new(1234567, 1024).encode().as_bytes(),
            b"/ack/1234567/1024/"
        );
    }
}
