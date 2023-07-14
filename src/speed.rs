pub mod event;
pub mod msg;
use std::array::TryFromSliceError;
use std::io;

#[derive(Debug)]
pub enum DecodeError {
    TooShort,
    IOError(io::Error),
    TryFromSliceError(TryFromSliceError),
}

impl From<TryFromSliceError> for DecodeError {
    fn from(error: TryFromSliceError) -> Self {
        DecodeError::TryFromSliceError(error)
    }
}

impl From<io::Error> for DecodeError {
    fn from(error: io::Error) -> Self {
        DecodeError::IOError(error)
    }
}

#[derive(Debug, PartialEq)]
pub enum DecodedMsg {
    Plate(msg::Plate),
    WantHeartbeat(msg::WantHeartbeat),
    IAmCamera(msg::IAmCamera),
    IAmDispatcher(msg::IAmDispatcher),
    Unknown,
}

impl DecodedMsg {
    pub fn size(&self) -> usize {
        match self {
            DecodedMsg::Plate(plate_msg) => plate_msg.size(),
            DecodedMsg::WantHeartbeat(_) => 5,
            DecodedMsg::IAmCamera(_) => 7,
            DecodedMsg::IAmDispatcher(dispatcher_msg) => dispatcher_msg.size(),
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

#[cfg(test)]
mod tests {
    use crate::speed::msg::IAmCamera;
    use crate::speed::msg::{IAmDispatcher, Plate, WantHeartbeat};
    use crate::speed::DecodeError::TooShort;
    use crate::speed::{decode_msg, DecodedMsg};

    #[test]
    fn decode_error_message_too_short() {
        let bytes = [0x80, 0x42, 0x00, 0x64, 0x00, 0x3c];
        let decoded = decode_msg(&bytes);
        assert!(decoded.is_err());
        assert!(matches!(decoded.unwrap_err(), TooShort));
    }

    #[test]
    fn decode_iam_camera() {
        let bytes = [0x80, 0x00, 0x42, 0x00, 0x64, 0x00, 0x3c];
        let decoded = decode_msg(&bytes);
        assert!(decoded.is_ok());
        assert_eq!(
            decoded.unwrap(),
            DecodedMsg::IAmCamera(IAmCamera {
                road: 66,
                mile: 100,
                limit: 60
            })
        );
    }

    #[test]
    fn decode_iam_dispatcher() {
        let bytes = [0x81, 0x03, 0x00, 0x42, 0x01, 0x70, 0x13, 0x88];
        let decoded = decode_msg(&bytes);
        assert!(decoded.is_ok());
        assert_eq!(
            decoded.unwrap(),
            DecodedMsg::IAmDispatcher(IAmDispatcher {
                roads: Vec::from([66, 368, 5000])
            })
        );
    }

    #[test]
    fn decode_want_heartbeat() {
        let bytes = [0x40, 0x00, 0x00, 0x00, 0x0a];
        let decoded = decode_msg(&bytes);
        assert!(decoded.is_ok());
        assert_eq!(
            decoded.unwrap(),
            DecodedMsg::WantHeartbeat(WantHeartbeat { interval: 10 })
        );
    }

    #[test]
    fn decode_plate() {
        let bytes = [0x20, 0x04, 0x55, 0x4e, 0x31, 0x58, 0x00, 0x00, 0x03, 0xe8];
        let decoded = decode_msg(&bytes);
        assert!(decoded.is_ok());
        assert_eq!(
            decoded.unwrap(),
            DecodedMsg::Plate(Plate {
                plate: "UN1X".to_owned(),
                ts: 1000
            })
        );
    }
}
