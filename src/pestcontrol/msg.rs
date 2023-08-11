use crate::pestcontrol;
use crate::pestcontrol::{Decodable, Error};

#[derive(PartialEq, Debug)]
pub enum Msg {
    Hello(Hello),
    Ok,
    Error(ErrorMsg),
    DialAuth(DialAuth),
    TargetPopulations(TargetPopulations),
}

impl Msg {
    fn valid_checksum(buf: &[u8]) -> bool {
        buf.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte)) == 0
    }

    fn compute_checksum(buf: &[u8]) -> u8 {
        let sum = buf.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
        0_u8.wrapping_sub(sum)
    }
}

impl Decodable for Msg {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self>
    where
        Self: Sized,
    {
        if !Msg::valid_checksum(buf) {
            return Err(Error::InvalidChecksum);
        }

        let code = buf[0];
        let body = &buf[5..buf.len() - 1];
        match code {
            0x50 => {
                let hello = Hello::decode(body)?;
                if hello.protocol != "pestcontrol" || hello.version != 1 {
                    return Err(Error::InvalidProtocol);
                }
                Ok(Msg::Hello(hello))
            }
            0x51 => Ok(Msg::Error(ErrorMsg::decode(body)?)),
            0x52 => Ok(Msg::Ok),
            0x53 => Ok(Msg::DialAuth(DialAuth::decode(body)?)),
            0x54 => Ok(Msg::TargetPopulations(TargetPopulations::decode(body)?)),
            _ => Err(Error::InvalidMessage),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let (code, message_body) = match self {
            Msg::Hello(hello) => (0x50, hello.encode()),
            Msg::Error(error_msg) => (0x51, error_msg.encode()),
            Msg::Ok => (0x52, Vec::new()),
            Msg::DialAuth(dial_auth) => (0x53, dial_auth.encode()),
            Msg::TargetPopulations(target_populations) => (0x54, target_populations.encode()),
        };

        let mut buf = Vec::from([code]);
        buf.extend_from_slice(&((message_body.len() + 6) as u32).to_be_bytes());
        buf.extend_from_slice(&message_body);
        buf.push(Msg::compute_checksum(&buf));
        buf
    }

    fn len(&self) -> usize {
        let body_size = match self {
            Msg::Hello(hello) => hello.len(),
            Msg::Error(error_msg) => error_msg.len(),
            Msg::Ok => 0,
            Msg::DialAuth(dial_auth) => dial_auth.len(),
            Msg::TargetPopulations(target_populations) => target_populations.len(),
        };
        6 + body_size
    }
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

impl Decodable for Hello {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let protocol = String::decode(buf)?;
        let version = u32::decode(&buf[4 + protocol.len()..])?;
        let hello = Self { protocol, version };
        if hello.len() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(hello)
    }

    fn encode(&self) -> Vec<u8> {
        let mut vec = self.protocol.encode();
        vec.extend_from_slice(&self.version.to_be_bytes());
        vec
    }

    fn len(&self) -> usize {
        Decodable::len(&self.protocol) + self.version.len()
    }
}

#[derive(PartialEq, Debug)]
pub struct ErrorMsg {
    message: String,
}

impl Decodable for ErrorMsg {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let message = String::decode(buf)?;
        let error_msg = Self { message };
        if error_msg.len() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(error_msg)
    }

    fn encode(&self) -> Vec<u8> {
        self.message.encode()
    }

    fn len(&self) -> usize {
        Decodable::len(&self.message)
    }
}

#[derive(PartialEq, Debug)]
pub struct DialAuth {
    site: u32,
}

impl Decodable for DialAuth {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let site = u32::decode(buf)?;
        let dial_auth = Self { site };
        if dial_auth.len() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(dial_auth)
    }

    fn encode(&self) -> Vec<u8> {
        Vec::from(self.site.to_be_bytes())
    }

    fn len(&self) -> usize {
        self.site.len()
    }
}

#[derive(PartialEq, Debug)]
pub struct TargetPopulations {
    site: u32,
    populations: Vec<pestcontrol::TargetPopulation>,
}

impl Decodable for TargetPopulations {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let site = u32::decode(buf)?;
        let populations = Vec::decode(&buf[4..])?;
        let target_populations = Self { site, populations };
        if target_populations.len() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(target_populations)
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::from(self.site.to_be_bytes());
        buf.extend_from_slice(&self.populations.encode());
        buf
    }

    fn len(&self) -> usize {
        self.site.len() + Decodable::len(&self.populations)
    }
}

#[cfg(test)]
mod tests {
    use crate::pestcontrol::msg::{DialAuth, ErrorMsg, TargetPopulations};
    use crate::pestcontrol::msg::{Hello, Msg};
    use crate::pestcontrol::{Decodable, TargetPopulation};

    #[test]
    fn encode_decode_hello() {
        let msg = Msg::Hello(Hello::default());
        let encoded = msg.encode();
        assert_eq!(Msg::decode(&encoded), Ok(msg))
    }

    #[test]
    fn encode_decode_error() {
        let msg = Msg::Error(ErrorMsg {
            message: "error".to_string(),
        });
        let encoded = msg.encode();
        assert_eq!(Msg::decode(&encoded), Ok(msg))
    }

    #[test]
    fn encode_decode_ok() {
        let msg = Msg::Ok;
        let encoded = msg.encode();
        assert_eq!(Msg::decode(&encoded), Ok(msg))
    }

    #[test]
    fn encode_decode_dial_auth() {
        let msg = Msg::DialAuth(DialAuth { site: 123 });
        let encoded = msg.encode();
        assert_eq!(Msg::decode(&encoded), Ok(msg))
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
        let encoded = msg.encode();
        assert_eq!(Msg::decode(&encoded), Ok(msg))
    }
}
