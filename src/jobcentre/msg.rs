use serde::{Deserialize, Serialize};
use serde_json::Value::Object;
use serde_json::{from_str, from_value, json, Value};

#[derive(Debug)]
pub enum Msg {
    Put(Put),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Put {
    queue: String,
    job: serde_json::Value,
    pri: u32,
}

#[derive(Debug)]
pub enum Error {
    Unknown,
    Serde(serde_json::Error),
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Serde(value)
    }
}

type Result<T> = std::result::Result<T, Error>;

pub fn parse(str: &str) -> Result<Msg> {
    if let Object(map) = from_str(str)? {
        match map.get("request") {
            Some(value) if *value == json!("put") => Ok(Msg::Put(from_value(Object(map))?)),
            _ => Err(Error::Unknown),
        }
    } else {
        Err(Error::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, Msg, Put};
    use crate::jobcentre::msg::Error;

    #[test]
    fn parse_put_msg() {
        let put_msg = r#"
           {"request": "put", "queue": "q1", "job": {}, "pri": 10}
        "#;
        assert!(matches!(parse(put_msg), Ok(Msg::Put(_))));
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
