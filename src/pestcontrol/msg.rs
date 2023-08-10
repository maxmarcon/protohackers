use crate::pestcontrol;
use crate::pestcontrol::{
    decode_str, decode_target_populations, decode_u32, encode_str, encode_target_populations,
    target_populations_len, Error,
};

#[derive(PartialEq, Debug)]
pub enum Msg {
    Hello(Hello),
    Ok,
    Error(ErrorMsg),
    DialAuth(DialAuth),
    TargetPopulations(TargetPopulations),
}

#[derive(PartialEq, Debug)]
pub struct Hello {
    protocol: String,
    version: u32,
}

impl Default for Hello {
    fn default() -> Self {
        Self {
            protocol: "pestcontrol".to_string(),
            version: 1,
        }
    }
}

impl Hello {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let protocol = decode_str(buf)?;
        let version = decode_u32(&buf[4 + protocol.len()..])?;
        if 5 + protocol.len() + 4 != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(Self { protocol, version })
    }

    fn encode(&self) -> Vec<u8> {
        let mut vec = encode_str(&self.protocol);
        vec.extend_from_slice(&self.version.to_be_bytes());
        vec
    }
}

#[derive(PartialEq, Debug)]
pub struct ErrorMsg {
    message: String,
}

impl ErrorMsg {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let message = decode_str(buf)?;
        if 5 + message.len() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(Self { message })
    }

    fn encode(&self) -> Vec<u8> {
        encode_str(&self.message)
    }
}

#[derive(PartialEq, Debug)]
pub struct DialAuth {
    site: u32,
}

impl DialAuth {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let site = decode_u32(buf)?;
        if buf.len() != 5 {
            return Err(Error::InvalidLength);
        }
        Ok(Self { site })
    }

    fn encode(&self) -> Vec<u8> {
        Vec::from(self.site.to_be_bytes())
    }
}

#[derive(PartialEq, Debug)]
pub struct TargetPopulations {
    site: u32,
    populations: Vec<pestcontrol::TargetPopulation>,
}

impl TargetPopulations {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let site = decode_u32(buf)?;
        let populations = decode_target_populations(&buf[4..])?;
        if 5 + target_populations_len(&populations) != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(Self { site, populations })
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::from(self.site.to_be_bytes());
        buf.extend_from_slice(&encode_target_populations(&self.populations));
        buf
    }
}

pub fn decode(buf: &[u8]) -> pestcontrol::Result<Msg> {
    if !valid_checksum(buf) {
        return Err(pestcontrol::Error::InvalidChecksum);
    }

    let code = buf[0];
    let body = &buf[5..];
    match code {
        0x50 => {
            let hello = Hello::decode(body)?;
            if hello.protocol != "pestcontrol" || hello.version != 1 {
                return Err(pestcontrol::Error::InvalidProtocol);
            }
            Ok(Msg::Hello(hello))
        }
        0x51 => Ok(Msg::Error(ErrorMsg::decode(body)?)),
        0x52 => Ok(Msg::Ok),
        0x53 => Ok(Msg::DialAuth(DialAuth::decode(body)?)),
        0x54 => Ok(Msg::TargetPopulations(TargetPopulations::decode(body)?)),
        _ => Err(pestcontrol::Error::InvalidMessage),
    }
}

pub fn encode(msg: &Msg) -> Vec<u8> {
    let (code, message_body) = match msg {
        Msg::Hello(hello) => (0x50, hello.encode()),
        Msg::Error(error_msg) => (0x51, error_msg.encode()),
        Msg::Ok => (0x52, Vec::new()),
        Msg::DialAuth(dial_auth) => (0x53, dial_auth.encode()),
        Msg::TargetPopulations(target_populations) => (0x54, target_populations.encode()),
    };

    let mut buf = Vec::from([code]);
    buf.extend_from_slice(&((message_body.len() + 6) as u32).to_be_bytes());
    buf.extend_from_slice(&message_body);
    buf.push(compute_checksum(&buf));
    buf
}

fn valid_checksum(buf: &[u8]) -> bool {
    buf.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte)) == 0
}

fn compute_checksum(buf: &[u8]) -> u8 {
    let sum = buf.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
    0_u8.wrapping_sub(sum)
}

#[cfg(test)]
mod tests {
    use crate::pestcontrol::msg::{decode, encode, Hello, Msg};
    use crate::pestcontrol::msg::{DialAuth, ErrorMsg, TargetPopulations};
    use crate::pestcontrol::TargetPopulation;

    #[test]
    fn encode_decode_hello() {
        let msg = Msg::Hello(Hello::default());
        let encoded = encode(&msg);
        assert_eq!(decode(&encoded), Ok(msg))
    }

    #[test]
    fn encode_decode_error() {
        let msg = Msg::Error(ErrorMsg {
            message: "error".to_string(),
        });
        let encoded = encode(&msg);
        assert_eq!(decode(&encoded), Ok(msg))
    }

    #[test]
    fn encode_decode_ok() {
        let msg = Msg::Ok;
        let encoded = encode(&msg);
        assert_eq!(decode(&encoded), Ok(msg))
    }

    #[test]
    fn encode_decode_dial_auth() {
        let msg = Msg::DialAuth(DialAuth { site: 123 });
        let encoded = encode(&msg);
        assert_eq!(decode(&encoded), Ok(msg))
    }

    #[test]
    fn encode_decode_target_populations() {
        let populations = vec![
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

        let msg = Msg::TargetPopulations(TargetPopulations {
            site: 123,
            populations,
        });
        let encoded = encode(&msg);
        assert_eq!(decode(&encoded), Ok(msg))
    }
}
