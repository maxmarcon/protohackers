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

pub fn encode_str(str: &str) -> Vec<u8> {
    let mut v = Vec::from((str.len() as u32).to_be_bytes());
    v.extend_from_slice(str.as_bytes());
    v
}

pub fn decode_str(buf: &[u8]) -> Result<String> {
    let strlen = decode_u32(buf)? as usize;
    if buf.len() < strlen + 4 {
        return Err(Error::InvalidLength);
    }
    Ok(String::from_utf8(buf[4..strlen + 4].to_vec())?)
}

pub fn decode_u32(buf: &[u8]) -> Result<u32> {
    if buf.len() < 4 {
        Err(Error::InvalidLength)
    } else {
        Ok(u32::from_be_bytes(buf[..4].try_into().unwrap()))
    }
}

pub fn target_populations_len(target_populations: &[TargetPopulation]) -> usize {
    4 + target_populations
        .iter()
        .map(|tp| 12 + tp.species.len())
        .sum::<usize>()
}

pub fn encode_target_populations(target_populations: &[TargetPopulation]) -> Vec<u8> {
    let mut buf = Vec::from((target_populations.len() as u32).to_be_bytes());
    for tp in target_populations {
        buf.extend_from_slice(&encode_str(&tp.species));
        buf.extend_from_slice(&tp.min.to_be_bytes());
        buf.extend_from_slice(&tp.max.to_be_bytes());
    }
    buf
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

pub fn encode_populations(populations: Vec<Population>) -> Vec<u8> {
    let mut buf = Vec::from((populations.len() as u32).to_be_bytes());
    for p in populations {
        buf.extend_from_slice(&encode_str(&p.species));
        buf.extend_from_slice(&p.count.to_be_bytes());
    }
    buf
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

#[cfg(test)]
mod tests {
    use super::{
        decode_populations, decode_str, decode_target_populations, encode_str, Population,
        TargetPopulation,
    };
    use crate::pestcontrol::{encode_populations, encode_target_populations};

    #[test]
    fn test_encode_decode_str() {
        let str = "hello my valentine!";
        let encoded = encode_str(str);

        assert_eq!(decode_str(&encoded), Ok(str.to_string()))
    }

    #[test]
    fn test_decode_encode_target_populations() {
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

        assert_eq!(encode_target_populations(&target_populations), bytes);
    }

    #[test]
    fn test_decode_encode_populations() {
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
                    count: 5,
                },
            ])
        );

        let populations = decoded.unwrap();

        assert_eq!(encode_populations(populations), bytes);
    }
}
