use crate::speed::DecodeError;

pub fn encode_str(msg: &str) -> Vec<u8> {
    let mut bytes = Vec::from([msg.len() as u8]);
    bytes.append(&mut Vec::from(msg.as_bytes()));
    bytes
}

pub fn decode_str(bytes: &[u8]) -> Result<(String, &[u8]), super::DecodeError> {
    if bytes.is_empty() {
        return Err(super::DecodeError::TooShort);
    }
    let strlen = bytes[0] as usize;
    if bytes.len() < strlen + 1 {
        return Err(super::DecodeError::TooShort);
    }
    Ok((
        String::from_utf8_lossy(&bytes[1..strlen + 1]).into(),
        &bytes[strlen + 1..],
    ))
}

pub fn decode_u16(bytes: &[u8]) -> Result<(u16, &[u8]), super::DecodeError> {
    let val = u16::from_be_bytes(bytes.try_into()?);
    Ok((val, &bytes[4..]))
}

pub fn decode_u32(bytes: &[u8]) -> Result<(u32, &[u8]), super::DecodeError> {
    let val = u32::from_be_bytes(bytes.try_into()?);
    Ok((val, &bytes[4..]))
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

pub struct Plate {
    plate: String,
    ts: u32,
}

impl Plate {
    pub const CODE: u8 = 0x20;

    pub fn decode(bytes: &[u8]) -> Result<Self, super::DecodeError> {
        let (plate, bytes) = decode_str(bytes)?;
        let (ts, _) = decode_u32(bytes)?;
        Ok(Self { plate, ts })
    }

    pub fn len(&self) -> usize {
        self.plate.len() + 6
    }
}

#[derive(Debug, Clone)]
pub struct Ticket {
    plate: String,
    road: u16,
    mile1: u16,
    ts1: u32,
    mile2: u16,
    ts2: u32,
    speed: u16,
}

impl Ticket {
    pub const CODE: u8 = 0x21;

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

pub struct WantHeartbeat {
    pub interval: u32,
}

impl WantHeartbeat {
    pub const CODE: u8 = 0x40;

    pub fn new(interval: u32) -> Self {
        Self { interval }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (interval, _) = decode_u32(bytes)?;
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
        let (road, bytes) = decode_u16(bytes)?;
        let (mile, bytes) = decode_u16(bytes)?;
        let (limit, _bytes) = decode_u16(bytes)?;
        Ok(Self { road, mile, limit })
    }
}

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
        let mut bytes = bytes;
        let mut road;
        for _ in 0..numroads {
            (road, bytes) = decode_u16(bytes)?;
            roads.push(road);
        }
        Ok(Self { roads: roads })
    }

    pub fn len(&self) -> usize {
        self.roads.len() * 2 + 2
    }
}
