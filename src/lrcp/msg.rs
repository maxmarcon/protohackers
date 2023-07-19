use std::io::Error;
use std::num::ParseIntError;
use std::str::FromStr;
use std::string::FromUtf8Error;

#[derive(Debug)]
pub enum DecodeError {
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
    Data(Data),
}

#[derive(Debug, PartialEq)]
pub struct Connect {
    pub session: i32,
}

impl Connect {
    pub fn new(session: i32) -> Self {
        Self { session }
    }
}

#[derive(Debug, PartialEq)]
pub struct Close {
    pub session: i32,
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
pub struct Ack {
    pub session: i32,
    pub length: i32,
}

impl Ack {
    pub fn new(session: i32, length: i32) -> Self {
        Self { session, length }
    }
    pub fn encode(&self) -> String {
        format!("/ack/{}/{}/", self.session, self.length)
    }
}

#[derive(Debug, PartialEq)]
pub struct Data {
    pub session: i32,
    pub pos: i32,
    pub data: String,
}

impl Data {
    pub fn new(session: i32, pos: i32, data: &str) -> Self {
        Self {
            session,
            pos,
            data: data.to_owned(),
        }
    }
    pub fn encode(&self) -> String {
        format!(
            "/data/{}/{}/{}/",
            self.session,
            self.pos,
            self.data.replace('\\', "\\\\").replace('/', "\\/")
        )
    }
}

const MAX_MSG_LEN: usize = 1000;

pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    if bytes.len() >= MAX_MSG_LEN {
        return Err(DecodeError::Invalid);
    }
    let str = String::from_utf8(bytes.into())?;
    if !str.starts_with('/') || !str.ends_with('/') {
        return Err(DecodeError::Invalid);
    }

    let mut str = str.trim_matches('/');
    let mut pieces = Vec::new();
    while let Some(sep) = str.find('/') {
        pieces.push(&str[..sep]);
        str = &str[sep + 1..];
        if pieces.len() == 3 {
            break;
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
        ["data", session, pos, data] => {
            if data
                .char_indices()
                .any(|(pos, ch)| ch == '/' && pos > 0 && &data[pos - 1..pos] != "\\")
            {
                Err(DecodeError::Invalid)
            } else {
                Ok(Decoded::Data(Data::new(
                    i32::from_str(session)?,
                    i32::from_str(pos)?,
                    &data.replace("\\\\", "\\").replace("\\/", "/"),
                )))
            }
        }
        _ => Err(DecodeError::Invalid),
    }
}

#[cfg(test)]
mod tests {
    use super::decode;
    use super::DecodeError;
    use super::Decoded;
    use crate::lrcp::msg::{Ack, Close, Connect, Data};

    #[test]
    fn decode_error_too_few_parts() {
        let decoded = decode(b"/connect/");
        assert!(decoded.is_err());
        let decoded = decoded.unwrap_err();
        assert!(matches!(decoded, DecodeError::Invalid));
    }

    #[test]
    fn decode_error_invalid_integer() {
        let decoded = decode(b"/connect/123\\/33/");
        assert!(decoded.is_err());
        let decoded = decoded.unwrap_err();
        assert!(matches!(decoded, DecodeError::Invalid));
    }

    #[test]
    fn decode_error_too_many_parts() {
        let decoded = decode(b"/data/1734379091/0/illegal data/has too many/parts/");
        assert!(decoded.is_err());
        let decoded = decoded.unwrap_err();
        assert!(matches!(decoded, DecodeError::Invalid));
    }

    #[test]
    fn decode_error_too_large_integer() {
        let decoded = decode(b"/data/2147483648/0/session number too long/");
        assert!(decoded.is_err());
        let decoded = decoded.unwrap_err();
        assert!(matches!(decoded, DecodeError::ParseIntError(_)));
    }

    #[test]
    fn decode_error_message_too_long() {
        let data: String = (1..1000).map(|_| 'X').collect();
        let decoded = decode(format!("/data/1734379091/0/{data}/").as_bytes());
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
        assert_eq!(Close::new(1234567).encode(), "/close/1234567/");
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
        assert_eq!(Ack::new(1234567, 1024).encode(), "/ack/1234567/1024/");
    }

    #[test]
    fn decode_data() {
        let decoded = decode(b"/data/1234567/1024/hello/");
        assert!(decoded.is_ok());
        let decoded = decoded.unwrap();
        assert_eq!(decoded, Decoded::Data(Data::new(1234567, 1024, "hello")));
    }

    #[test]
    fn encode_data() {
        assert_eq!(
            Data::new(1234567, 1024, "hello").encode(),
            "/data/1234567/1024/hello/"
        );
    }

    #[test]
    fn decode_escaped_data() {
        let decoded = decode(b"/data/1234567/1024/hel\\/lo\\\\/");
        assert!(decoded.is_ok());
        let decoded = decoded.unwrap();
        assert_eq!(decoded, Decoded::Data(Data::new(1234567, 1024, "hel/lo\\")));
    }

    #[test]
    fn encode_escaped_data() {
        assert_eq!(
            Data::new(1234567, 1024, "hel/lo\\").encode(),
            "/data/1234567/1024/hel\\/lo\\\\/"
        );
    }
}
