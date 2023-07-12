use std::array::TryFromSliceError;
use std::io;

#[derive(Debug)]
pub enum Error {
    InvalidMsg,
    IOError(io::Error),
}

impl From<TryFromSliceError> for Error {
    fn from(_value: TryFromSliceError) -> Self {
        Error::InvalidMsg
    }
}

pub mod Msg {
    pub fn encode_str(msg: &str) -> Vec<u8> {
        let mut bytes = Vec::from([msg.len() as u8]);
        bytes.append(&mut Vec::from(msg.as_bytes()));
        bytes
    }

    pub fn decode_str(bytes: &[u8]) -> Result<(String, &[u8]), super::Error> {
        if bytes.is_empty() {
            return Err(super::Error::InvalidMsg);
        }
        let strlen = bytes[0] as usize;
        if bytes.len() < strlen + 1 {
            return Err(super::Error::InvalidMsg);
        }
        Ok((
            String::from_utf8_lossy(&bytes[1..strlen + 1]).into(),
            &bytes[strlen + 1..],
        ))
    }

    pub fn decode_u16(bytes: &[u8]) -> Result<(u16, &[u8]), super::Error> {
        let val = u16::from_be_bytes(bytes.try_into()?);
        Ok((val, &bytes[4..]))
    }

    pub fn decode_u32(bytes: &[u8]) -> Result<(u32, &[u8]), super::Error> {
        let val = u32::from_be_bytes(bytes.try_into()?);
        Ok((val, &bytes[4..]))
    }

    struct Error {
        msg: String,
    }

    impl Error {
        const CODE: u8 = 0x10;

        pub fn new(msg: String) -> Self {
            Self { msg }
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut bytes = Vec::from([Error::CODE]);

            bytes.append(&mut encode_str(&self.msg));
            bytes
        }
    }

    struct Plate {
        plate: String,
        ts: u32,
    }

    impl Plate {
        const CODE: u8 = 0x20;

        pub fn decode(bytes: &[u8]) -> Result<Self, super::Error> {
            let (plate, bytes) = decode_str(bytes)?;
            let (ts, _) = decode_u32(bytes)?;
            Ok(Self { plate, ts })
        }
    }

    struct Ticket {
        plate: String,
        road: u16,
        mile1: u16,
        ts1: u32,
        mile2: u16,
        ts2: u32,
        speed: u16,
    }

    impl Ticket {
        const CODE: u8 = 0x21;

        pub fn new(
            plate: String,
            road: u16,
            mile1: u16,
            ts1: u32,
            mile2: u16,
            ts2: u32,
            speed: u16,
        ) -> Self {
            Self {
                plate,
                road,
                mile1,
                ts1,
                mile2,
                ts2,
                speed,
            }
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut bytes = Vec::from([Ticket::CODE]);
            bytes.append(&mut encode_str(&self.plate));
            bytes.append(&mut self.road.to_be_bytes().to_vec());
            bytes.append(&mut self.mile1.to_be_bytes().to_vec());
            bytes.append(&mut self.ts1.to_be_bytes().to_vec());
            bytes.append(&mut self.mile2.to_be_bytes().to_vec());
            bytes.append(&mut self.ts2.to_be_bytes().to_vec());
            bytes.append(&mut self.speed.to_be_bytes().to_vec());
            bytes
        }
    }
}
