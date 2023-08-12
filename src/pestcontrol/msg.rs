use crate::pestcontrol;
use crate::pestcontrol::{Action, Decodable, Error, Population};

#[derive(PartialEq, Debug, Clone)]
pub enum Msg {
    Hello(Hello),
    Ok,
    Error(ErrorMsg),
    DialAuth(DialAuth),
    TargetPopulations(TargetPopulations),
    CreatePolicy(CreatePolicy),
    DeletePolicy(Policy),
    PolicyResult(Policy),
    SiteVisit(SiteVisit),
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
            0x55 => Ok(Msg::CreatePolicy(CreatePolicy::decode(body)?)),
            0x56 => Ok(Msg::DeletePolicy(Policy::decode(body)?)),
            0x57 => Ok(Msg::PolicyResult(Policy::decode(body)?)),
            0x58 => Ok(Msg::SiteVisit(SiteVisit::decode(body)?)),
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
            Msg::CreatePolicy(create_policy) => (0x55, create_policy.encode()),
            Msg::DeletePolicy(policy) => (0x56, policy.encode()),
            Msg::PolicyResult(policy) => (0x57, policy.encode()),
            Msg::SiteVisit(site_visit) => (0x58, site_visit.encode()),
        };

        let mut buf = Vec::from([code]);
        buf.extend_from_slice(&((message_body.len() + 6) as u32).to_be_bytes());
        buf.extend_from_slice(&message_body);
        buf.push(Msg::compute_checksum(&buf));
        buf
    }

    fn bytelen(&self) -> usize {
        let body_size = match self {
            Msg::Hello(hello) => hello.bytelen(),
            Msg::Error(error_msg) => error_msg.bytelen(),
            Msg::Ok => 0,
            Msg::DialAuth(dial_auth) => dial_auth.bytelen(),
            Msg::TargetPopulations(target_populations) => target_populations.bytelen(),
            Msg::CreatePolicy(create_policy) => create_policy.bytelen(),
            Msg::DeletePolicy(policy) => policy.bytelen(),
            Msg::PolicyResult(policy) => policy.bytelen(),
            Msg::SiteVisit(site_visit) => site_visit.bytelen(),
        };
        6 + body_size
    }
}

#[derive(PartialEq, Debug, Clone)]
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
        if hello.bytelen() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(hello)
    }

    fn encode(&self) -> Vec<u8> {
        let mut vec = self.protocol.encode();
        vec.extend_from_slice(&self.version.to_be_bytes());
        vec
    }

    fn bytelen(&self) -> usize {
        Decodable::bytelen(&self.protocol) + self.version.bytelen()
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct ErrorMsg {
    message: String,
}

impl ErrorMsg {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl From<&Error> for ErrorMsg {
    fn from(value: &Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl Decodable for ErrorMsg {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let message = String::decode(buf)?;
        let error_msg = Self { message };
        if error_msg.bytelen() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(error_msg)
    }

    fn encode(&self) -> Vec<u8> {
        self.message.encode()
    }

    fn bytelen(&self) -> usize {
        Decodable::bytelen(&self.message)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct DialAuth {
    site: u32,
}

impl Decodable for DialAuth {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let site = u32::decode(buf)?;
        let dial_auth = Self { site };
        if dial_auth.bytelen() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(dial_auth)
    }

    fn encode(&self) -> Vec<u8> {
        Vec::from(self.site.to_be_bytes())
    }

    fn bytelen(&self) -> usize {
        self.site.bytelen()
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct TargetPopulations {
    site: u32,
    populations: Vec<pestcontrol::TargetPopulation>,
}

impl Decodable for TargetPopulations {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self> {
        let site = u32::decode(buf)?;
        let populations = Vec::decode(&buf[4..])?;
        let target_populations = Self { site, populations };
        if target_populations.bytelen() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(target_populations)
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::from(self.site.to_be_bytes());
        buf.extend_from_slice(&self.populations.encode());
        buf
    }

    fn bytelen(&self) -> usize {
        self.site.bytelen() + Decodable::bytelen(&self.populations)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct CreatePolicy {
    species: String,
    action: Action,
}

impl Decodable for CreatePolicy {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self>
    where
        Self: Sized,
    {
        let species = String::decode(buf)?;
        let action = Action::decode(&buf[species.bytelen()..])?;
        let create_policy = Self { species, action };
        if create_policy.bytelen() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(create_policy)
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = self.species.encode();
        buf.extend_from_slice(&self.action.encode());
        buf
    }

    fn bytelen(&self) -> usize {
        self.species.bytelen() + self.action.bytelen()
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct Policy {
    policy: u32,
}

impl Decodable for Policy {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self>
    where
        Self: Sized,
    {
        let delete_policy = Self {
            policy: u32::decode(buf)?,
        };
        if delete_policy.bytelen() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(delete_policy)
    }

    fn encode(&self) -> Vec<u8> {
        self.policy.encode()
    }

    fn bytelen(&self) -> usize {
        self.policy.bytelen()
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct SiteVisit {
    pub site: u32,
    populations: Vec<Population>,
}

impl Decodable for SiteVisit {
    fn decode(buf: &[u8]) -> pestcontrol::Result<Self>
    where
        Self: Sized,
    {
        let site = u32::decode(buf)?;
        let populations = Vec::decode(&buf[site.bytelen()..])?;
        let site_visit = Self { site, populations };
        if site_visit.bytelen() != buf.len() {
            return Err(Error::InvalidLength);
        }
        Ok(site_visit)
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = self.site.encode();
        buf.extend_from_slice(&self.populations.encode());
        buf
    }

    fn bytelen(&self) -> usize {
        self.site.bytelen() + self.populations.bytelen()
    }
}

#[cfg(test)]
mod tests {
    use crate::pestcontrol::msg::{
        CreatePolicy, DialAuth, ErrorMsg, Policy, SiteVisit, TargetPopulations,
    };
    use crate::pestcontrol::msg::{Hello, Msg};
    use crate::pestcontrol::{Action, Decodable, Population, TargetPopulation};

    #[test]
    fn encode_decode_hello() {
        let msg = Hello::default();
        let encoded = Msg::Hello(msg.clone()).encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::Hello(m)) if m == msg));
    }

    #[test]
    fn encode_decode_error() {
        let msg = ErrorMsg {
            message: "error".to_string(),
        };
        let encoded = Msg::Error(msg.clone()).encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::Error(m)) if m == msg));
    }

    #[test]
    fn encode_decode_ok() {
        let encoded = Msg::Ok.encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::Ok)));
    }

    #[test]
    fn encode_decode_dial_auth() {
        let msg = DialAuth { site: 123 };
        let encoded = Msg::DialAuth(msg.clone()).encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::DialAuth(m)) if m == msg));
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

        let msg = TargetPopulations {
            site: 123,
            populations,
        };
        let encoded = Msg::TargetPopulations(msg.clone()).encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::TargetPopulations(m)) if m == msg));
    }

    #[test]
    fn encode_decode_create_polocy() {
        let msg = CreatePolicy {
            species: "dog".to_string(),
            action: Action::Cull,
        };
        let encoded = Msg::CreatePolicy(msg.clone()).encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::CreatePolicy(m)) if m == msg));
    }

    #[test]
    fn encode_decode_delete_policy() {
        let msg = Policy { policy: 123 };
        let encoded = Msg::DeletePolicy(msg.clone()).encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::DeletePolicy(m)) if m == msg));
    }

    #[test]
    fn encode_decode_policy_result() {
        let msg = Policy { policy: 123 };
        let encoded = Msg::PolicyResult(msg.clone()).encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::PolicyResult(m)) if m == msg));
    }

    #[test]
    fn encode_decode_site_visit() {
        let msg = SiteVisit {
            site: 123,
            populations: vec![
                Population {
                    species: "dog".to_string(),
                    count: 30,
                },
                Population {
                    species: "rat".to_string(),
                    count: 3000,
                },
            ],
        };
        let encoded = Msg::SiteVisit(msg.clone()).encode();
        assert!(matches!(Msg::decode(&encoded), Ok(Msg::SiteVisit(m)) if m == msg));
    }
}
