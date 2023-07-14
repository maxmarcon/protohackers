use crate::speed::DecodeError;
use std::collections::HashSet;
use std::ops::RangeInclusive;

pub fn encode_str(msg: &str) -> Vec<u8> {
    let mut bytes = Vec::from([msg.len() as u8]);
    bytes.append(&mut Vec::from(msg.as_bytes()));
    bytes
}

pub fn decode_str(bytes: &[u8]) -> Result<String, DecodeError> {
    if bytes.is_empty() {
        return Err(DecodeError::TooShort);
    }
    let strlen = bytes[0] as usize;
    assert_min_len(&bytes[1..], strlen)?;
    Ok(String::from_utf8_lossy(&bytes[1..strlen + 1]).into())
}

pub fn decode_u16(bytes: &[u8]) -> Result<u16, DecodeError> {
    assert_min_len(bytes, 2)?;
    Ok(u16::from_be_bytes(bytes[..2].try_into()?))
}

pub fn decode_u32(bytes: &[u8]) -> Result<u32, DecodeError> {
    assert_min_len(bytes, 4)?;
    Ok(u32::from_be_bytes(bytes[..4].try_into()?))
}

fn assert_min_len(bytes: &[u8], len: usize) -> Result<(), DecodeError> {
    if bytes.len() < len {
        Err(DecodeError::TooShort)
    } else {
        Ok(())
    }
}

pub struct Error {
    msg: String,
}

impl Error {
    pub const CODE: u8 = 0x10;

    pub fn new(msg: &str) -> Self {
        Self {
            msg: msg.to_owned(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::from([Error::CODE]);

        bytes.append(&mut encode_str(&self.msg));
        bytes
    }
}

#[derive(Debug, PartialEq)]
pub struct Plate {
    pub plate: String,
    pub ts: u32,
}

impl Plate {
    pub const CODE: u8 = 0x20;

    pub fn decode(bytes: &[u8]) -> Result<Self, super::DecodeError> {
        let plate = decode_str(bytes)?;
        let ts = decode_u32(&bytes[plate.len() + 1..])?;
        Ok(Self { plate, ts })
    }

    pub fn len(&self) -> usize {
        self.plate.len() + 6
    }
}

#[derive(Debug, Clone)]
pub struct Ticket {
    pub plate: String,
    pub road: u16,
    pub mile1: u16,
    pub ts1: u32,
    pub mile2: u16,
    pub ts2: u32,
    pub speed: u16,
}

impl Ticket {
    pub const CODE: u8 = 0x21;

    pub fn new(
        plate: &str,
        road: u16,
        mile1: u16,
        ts1: u32,
        mile2: u16,
        ts2: u32,
        speed: u16,
    ) -> Self {
        Self {
            plate: plate.to_owned(),
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

    pub fn overlaps(&self, days: &HashSet<u32>) -> bool {
        self.days().any(|day| days.contains(&day))
    }

    pub fn record(&self, days: &mut HashSet<u32>) {
        self.days().for_each(|day| {
            days.insert(day);
        })
    }

    fn days(&self) -> RangeInclusive<u32> {
        self.ts1 / 86400..=self.ts2 / 86400
    }
}

#[derive(Debug, PartialEq)]
pub struct WantHeartbeat {
    pub interval: u32,
}

impl WantHeartbeat {
    pub const CODE: u8 = 0x40;

    pub fn new(interval: u32) -> Self {
        Self { interval }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let interval = decode_u32(&bytes[..4])?;
        Ok(Self { interval })
    }
}

pub struct Heartbeat {}

impl Heartbeat {
    pub const CODE: u8 = 0x41;

    pub fn encode() -> Vec<u8> {
        Vec::from([Heartbeat::CODE])
    }
}

#[derive(Debug, PartialEq)]
pub struct IAmCamera {
    pub road: u16,
    pub mile: u16,
    pub limit: u16,
}

impl IAmCamera {
    pub const CODE: u8 = 0x80;

    pub fn new(road: u16, mile: u16, limit: u16) -> Self {
        Self { road, mile, limit }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let road = decode_u16(bytes)?;
        let mile = decode_u16(&bytes[2..])?;
        let limit = decode_u16(&bytes[4..])?;
        Ok(Self { road, mile, limit })
    }
}

#[derive(Debug, PartialEq)]
pub struct IAmDispatcher {
    pub roads: Vec<u16>,
}

impl IAmDispatcher {
    pub const CODE: u8 = 0x81;

    pub fn decode(bytes: &[u8]) -> Result<Self, super::DecodeError> {
        if bytes.is_empty() {
            return Err(super::DecodeError::TooShort);
        }
        let numroads = bytes[0] as usize;
        let mut roads = Vec::with_capacity(numroads);
        let mut road;
        for idx in 0..numroads {
            road = decode_u16(&bytes[1 + (idx * 2)..])?;
            roads.push(road);
        }
        Ok(Self { roads })
    }

    pub fn len(&self) -> usize {
        self.roads.len() * 2 + 2
    }
}
