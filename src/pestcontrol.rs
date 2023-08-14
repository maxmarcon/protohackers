use std::fmt::{Display, Formatter};
use std::io;
use std::io::ErrorKind;
use std::string::FromUtf8Error;

pub mod msg;

#[derive(Debug)]
pub enum Error {
    InvalidMessage,
    InvalidChecksum,
    InvalidLength,
    InvalidProtocol,
    InvalidAction,
    TooLarge,
    Unexpected,
    FromUtf8Error(FromUtf8Error),
    IO(io::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidMessage => write!(f, "invalid message"),
            Error::InvalidChecksum => write!(f, "invalid checksum"),
            Error::InvalidLength => write!(f, "invalid message length"),
            Error::InvalidProtocol => write!(f, "invalid protocol"),
            Error::InvalidAction => write!(f, "invalid action"),
            Error::TooLarge => write!(f, "message too large"),
            Error::Unexpected => write!(f, "unexpected message"),
            Error::FromUtf8Error(error) => write!(f, "{error}"),
            Error::IO(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for io::Error {
    fn from(value: Error) -> Self {
        match value {
            Error::IO(error) => error,
            error => io::Error::new(ErrorKind::Other, error),
        }
    }
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Error::FromUtf8Error(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::IO(value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Decodable {
    fn decode(buf: &[u8]) -> Result<Self>
    where
        Self: Sized;

    fn encode(&self) -> Vec<u8>;

    fn bytelen(&self) -> usize;
}

#[derive(PartialEq, Debug, Clone)]
pub struct Population {
    pub species: String,
    pub count: u32,
}

#[derive(PartialEq, Debug, Clone)]
pub struct TargetPopulation {
    pub species: String,
    pub min: u32,
    pub max: u32,
}

#[derive(PartialEq, Debug, Clone)]
pub enum Action {
    Cull,
    Conserve,
}

impl Decodable for Action {
    fn decode(buf: &[u8]) -> Result<Self>
    where
        Self: Sized,
    {
        if buf.is_empty() {
            return Err(Error::InvalidLength);
        }
        match buf[0] {
            0x90 => Ok(Action::Cull),
            0xa0 => Ok(Action::Conserve),
            _ => Err(Error::InvalidAction),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let byte = match self {
            Action::Cull => 0x90,
            Action::Conserve => 0xa0,
        };
        Vec::from([byte])
    }

    fn bytelen(&self) -> usize {
        1
    }
}

impl Decodable for String {
    fn decode(buf: &[u8]) -> Result<Self>
    where
        Self: Sized,
    {
        let strlen = u32::decode(buf)? as usize;
        if buf.len() < strlen + 4 {
            return Err(Error::InvalidLength);
        }
        Ok(String::from_utf8(buf[4..strlen + 4].to_vec())?)
    }

    fn encode(&self) -> Vec<u8> {
        let mut v = Vec::from((self.len() as u32).to_be_bytes());
        v.extend_from_slice(self.as_bytes());
        v
    }

    fn bytelen(&self) -> usize {
        4 + self.len()
    }
}

impl Decodable for u32 {
    fn decode(buf: &[u8]) -> Result<Self>
    where
        Self: Sized,
    {
        if buf.len() < 4 {
            Err(Error::InvalidLength)
        } else {
            Ok(u32::from_be_bytes(buf[..4].try_into().unwrap()))
        }
    }

    fn encode(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }

    fn bytelen(&self) -> usize {
        4
    }
}

impl Decodable for Vec<TargetPopulation> {
    fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < 4 {
            return Err(Error::InvalidLength);
        }
        let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;
        let mut target_populations = Vec::new();
        let mut cur = 4;
        for _ in 0..len {
            let species = String::decode(&buf[cur..])?;
            cur += 4 + species.len();
            let min = u32::decode(&buf[cur..])?;
            cur += 4;
            let max = u32::decode(&buf[cur..])?;
            cur += 4;
            target_populations.push(TargetPopulation { species, min, max });
        }
        Ok(target_populations)
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::from((self.len() as u32).to_be_bytes());
        for tp in self {
            buf.extend_from_slice(&tp.species.encode());
            buf.extend_from_slice(&tp.min.to_be_bytes());
            buf.extend_from_slice(&tp.max.to_be_bytes());
        }
        buf
    }

    fn bytelen(&self) -> usize {
        4 + self.iter().map(|tp| 12 + tp.species.len()).sum::<usize>()
    }
}

impl Decodable for Vec<Population> {
    fn decode(buf: &[u8]) -> Result<Self>
    where
        Self: Sized,
    {
        if buf.len() < 4 {
            return Err(Error::InvalidLength);
        }
        let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;
        let mut populations = Vec::new();
        let mut cur = 4;
        for _ in 0..len {
            let species = String::decode(&buf[cur..])?;
            cur += 4 + species.len();
            let count = u32::decode(&buf[cur..])?;
            cur += 4;
            populations.push(Population { species, count });
        }
        Ok(populations)
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::from((self.len() as u32).to_be_bytes());
        for p in self {
            buf.extend_from_slice(&p.species.encode());
            buf.extend_from_slice(&p.count.to_be_bytes());
        }
        buf
    }

    fn bytelen(&self) -> usize {
        4 + self.iter().map(|p| 8 + p.species.len()).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::{Population, TargetPopulation};
    use crate::pestcontrol;
    use crate::pestcontrol::Decodable;

    #[test]
    fn test_encode_decode_string() {
        let str = "hello my valentine!".to_string();
        let encoded = str.encode();

        assert!(matches!(String::decode(&encoded), Ok(s) if s == str))
    }

    #[test]
    fn test_decode_encode_target_populations() {
        let target_populations = vec![
            TargetPopulation {
                species: "dog".to_string(),
                min: 1,
                max: 3,
            },
            TargetPopulation {
                species: "rat".to_string(),
                min: 0,
                max: 10,
            },
        ];

        let encoded = target_populations.encode();

        let decoded: pestcontrol::Result<Vec<TargetPopulation>> = Vec::decode(&encoded);

        assert!(matches!(
            decoded,
            Ok(tp) if tp == target_populations
        ));
    }

    #[test]
    fn test_decode_encode_populations() {
        let populations = vec![
            Population {
                species: "dog".to_string(),
                count: 1,
            },
            Population {
                species: "rat".to_string(),
                count: 5,
            },
        ];

        let encoded = populations.encode();

        let decoded: pestcontrol::Result<Vec<Population>> = Vec::decode(&encoded);

        assert!(matches!(
            decoded,
            Ok(p) if p == populations
        ));
    }
}
