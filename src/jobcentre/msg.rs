use serde::{Deserialize, Serialize};
use serde_json::Value::Object;
use serde_json::{from_str, json, Value};

#[derive(Debug)]
pub enum Msg {
    P(Put),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Put {
    queue: String,
    job: serde_json::Value,
    pri: u32,
}

#[derive(Debug)]
enum Error {
    Unknown,
    Serde(serde_json::Error),
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Serde(value)
    }
}

type Result<T> = std::result::Result<T, Error>;

fn parse(str: &str) -> Result<Msg> {
    if let Object(map) = from_str(str)? {
        let msg = match map.get("request") {
            Some(value) if *value == json!("put") => Msg::P(from_str::<Put>(str)?),
            _ => return Err(Error::Unknown),
        };
        return Ok(msg);
    }
    Err(Error::Unknown)
}

#[cfg(test)]
mod tests {
    use super::{parse, Msg, Put};
    use crate::jobcentre::msg::Error;
    use serde_json::json;

    #[test]
    fn parse_put_msg() {
        let put_msg = r#"
           {"request": "put", "queue": "q1", "job": {}, "pri": 10}
        "#;
        assert!(matches!(parse(put_msg), Ok(Msg::P(_))));
    }

    #[test]
    fn parse_invalid_msg() {
        let invalid_msg = r#"
           {"request": "put", "job": {}, "pri": 10}
        "#;
        assert!(matches!(parse(invalid_msg), Err(Error::Serde(_))));
    }

    #[test]
    fn parse_unknown_msg() {
        let invalid_msg = r#"
           {"request": "unknown", "job": {}, "pri": 10}
        "#;
        assert!(matches!(parse(invalid_msg), Err(Error::Unknown)));
    }
}
