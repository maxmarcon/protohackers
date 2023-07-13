pub mod event;
pub mod msg;
use std::array::TryFromSliceError;
use std::io;

#[derive(Debug)]
pub enum DecodeError {
    TooShort,
    IOError(io::Error),
}

impl From<TryFromSliceError> for DecodeError {
    fn from(_error: TryFromSliceError) -> Self {
        DecodeError::TooShort
    }
}

impl From<io::Error> for DecodeError {
    fn from(error: io::Error) -> Self {
        DecodeError::IOError(error)
    }
}

pub enum DecodedMsg {
    Plate(msg::Plate),
    WantHeartbeat(msg::WantHeartbeat),
    IAmCamera(msg::IAmCamera),
    IAmDispatcher(msg::IAmDispatcher),
    Unknown,
}

impl DecodedMsg {
    pub fn len(&self) -> usize {
        match self {
            DecodedMsg::Plate(plate_msg) => plate_msg.len(),
            DecodedMsg::WantHeartbeat(_) => 5,
            DecodedMsg::IAmCamera(_) => 7,
            DecodedMsg::IAmDispatcher(dispatcher_msg) => dispatcher_msg.len(),
            DecodedMsg::Unknown => 0,
        }
    }
}

pub fn decode_msg(bytes: &[u8]) -> Result<DecodedMsg, DecodeError> {
    if bytes.is_empty() {
        return Err(DecodeError::TooShort);
    }
    match bytes[0] {
        msg::Plate::CODE => msg::Plate::decode(&bytes[1..]).map(DecodedMsg::Plate),
        msg::WantHeartbeat::CODE => {
            msg::WantHeartbeat::decode(&bytes[1..]).map(DecodedMsg::WantHeartbeat)
        }
        msg::IAmCamera::CODE => msg::IAmCamera::decode(&bytes[1..]).map(DecodedMsg::IAmCamera),
        msg::IAmDispatcher::CODE => {
            msg::IAmDispatcher::decode(&bytes[1..]).map(DecodedMsg::IAmDispatcher)
        }
        _ => Ok(DecodedMsg::Unknown),
    }
}
