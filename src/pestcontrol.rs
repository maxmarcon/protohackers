use std::string::FromUtf8Error;

pub mod msg;

#[derive(PartialEq, Debug)]
pub enum Error {
    InvalidMessage,
    InvalidChecksum,
    InvalidLength,
    InvalidProtocol,
    FromUtf8Error(FromUtf8Error),
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Error::FromUtf8Error(value)
    }
}

type Result<T> = std::result::Result<T, Error>;

trait Decodable {
    fn decode(buf: &[u8]) -> Result<Self>
    where
        Self: Sized;

    fn encode(&self) -> Vec<u8>;

    fn len(&self) -> usize;
}

#[derive(PartialEq, Debug)]
pub struct Population {
    species: String,
    count: u32,
}

#[derive(PartialEq, Debug)]
pub struct TargetPopulation {
    species: String,
    min: u32,
    max: u32,
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

    fn len(&self) -> usize {
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

    fn len(&self) -> usize {
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

    fn len(&self) -> usize {
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

    fn len(&self) -> usize {
        4 + self.iter().map(|p| 8 + p.species.len()).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::{Population, TargetPopulation};
    use crate::pestcontrol::Decodable;

    #[test]
    fn test_encode_decode_string() {
        let str = "hello my valentine!".to_string();
        let encoded = str.encode();

        assert_eq!(String::decode(&encoded), Ok(str.to_string()))
    }

    #[test]
    fn test_decode_encode_target_populations() {
        let bytes = [
            0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x64, 0x6f, 0x67, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x03, 0x72, 0x61, 0x74, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x0a,
        ];
        let decoded = Vec::decode(&bytes);

        assert_eq!(
            decoded,
            Ok(vec![
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
            ])
        );

        let target_populations = decoded.unwrap();

        assert_eq!(target_populations.encode(), bytes);
    }

    #[test]
    fn test_decode_encode_populations() {
        let bytes = [
            0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x64, 0x6f, 0x67, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x03, 0x72, 0x61, 0x74, 0x00, 0x00, 0x00, 0x05,
        ];

        let decoded = Vec::decode(&bytes);

        assert_eq!(
            decoded,
            Ok(vec![
                Population {
                    species: "dog".to_string(),
                    count: 1,
                },
                Population {
                    species: "rat".to_string(),
                    count: 5,
                },
            ])
        );

        let populations = decoded.unwrap();

        assert_eq!(populations.encode(), bytes);
    }
}
