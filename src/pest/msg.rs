use std::string::FromUtf8Error;
use std::usize;

pub enum Msg {}

#[derive(PartialEq, Debug)]
pub enum Error {
    InvalidMessage,
    InvalidChecksum,
    InvalidLength,
    FromUtf8Error(FromUtf8Error),
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Error::FromUtf8Error(value)
    }
}

type Result<T> = std::result::Result<T, Error>;

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

pub struct Hello {}

impl Hello {
    fn decode(buf: &[u8]) -> Result<Hello> {
        todo!()
    }

    fn encode() -> Vec<u8> {
        todo!()
    }
}

pub fn decode(buf: &[u8]) -> Result<Msg> {
    todo!()
}

pub fn decode_u32(buf: &[u8]) -> Result<u32> {
    if buf.len() < 4 {
        Err(Error::InvalidLength)
    } else {
        Ok(u32::from_be_bytes(buf[..4].try_into().unwrap()))
    }
}

pub fn encode_str(str: &str) -> Vec<u8> {
    let mut v = Vec::from((str.len() as u32).to_be_bytes());
    v.extend_from_slice(&str.as_bytes());
    v
}

pub fn decode_str(buf: &[u8]) -> Result<String> {
    let strlen = decode_u32(buf)? as usize;
    if buf.len() < strlen + 4 {
        return Err(Error::InvalidLength);
    }
    Ok(String::from_utf8(buf[4..strlen + 4].to_vec())?)
}

pub fn decode_populations(buf: &[u8]) -> Result<Vec<Population>> {
    if buf.len() < 4 {
        return Err(Error::InvalidLength);
    }
    let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;
    let mut populations = Vec::new();
    let mut cur = 4;
    for _ in 0..len {
        let species = decode_str(&buf[cur..])?;
        cur += 4 + species.len();
        let count = decode_u32(&buf[cur..])?;
        cur += 4;
        populations.push(Population { species, count });
    }
    Ok(populations)
}

pub fn decode_target_populations(buf: &[u8]) -> Result<Vec<TargetPopulation>> {
    if buf.len() < 4 {
        return Err(Error::InvalidLength);
    }
    let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;
    let mut target_populations = Vec::new();
    let mut cur = 4;
    for _ in 0..len {
        let species = decode_str(&buf[cur..])?;
        cur += 4 + species.len();
        let min = decode_u32(&buf[cur..])?;
        cur += 4;
        let max = decode_u32(&buf[cur..])?;
        cur += 4;
        target_populations.push(TargetPopulation { species, min, max });
    }
    Ok(target_populations)
}

#[cfg(test)]
mod tests {
    use crate::pest::msg::decode_target_populations;
    use crate::pest::msg::encode_str;
    use crate::pest::msg::TargetPopulation;
    use crate::pest::msg::{decode_populations, decode_str, Population};

    #[test]
    fn test_encode_decode_str() {
        let str = "hello my valentine!";
        let encoded = encode_str(str);

        assert_eq!(decode_str(&encoded), Ok(str.to_string()))
    }

    #[test]
    fn test_decode_target_populations() {
        let bytes = [
            0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x64, 0x6f, 0x67, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x03, 0x72, 0x61, 0x74, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x0a,
        ];
        let decoded = decode_target_populations(&bytes);

        assert_eq!(
            decoded,
            Ok(vec![
                TargetPopulation {
                    species: "dog".to_string(),
                    min: 1,
                    max: 3
                },
                TargetPopulation {
                    species: "rat".to_string(),
                    min: 0,
                    max: 10
                }
            ])
        );
    }

    #[test]
    fn test_decode_populations() {
        let bytes = [
            0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x64, 0x6f, 0x67, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x03, 0x72, 0x61, 0x74, 0x00, 0x00, 0x00, 0x05,
        ];

        let decoded = decode_populations(&bytes);

        assert_eq!(
            decoded,
            Ok(vec![
                Population {
                    species: "dog".to_string(),
                    count: 1,
                },
                Population {
                    species: "rat".to_string(),
                    count: 5
                }
            ])
        );
    }
}
