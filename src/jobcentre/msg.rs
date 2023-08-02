use crate::jobcentre::msg::Error::Utf8Error;
use crate::jobcentre::Job;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, from_value, json, to_string, Value};
use std::fmt::{Display, Formatter};
use std::io;
use std::string::FromUtf8Error;

#[derive(Debug)]
pub enum Msg {
    Put(Put),
    Get(Get),
    Delete(Delete),
    Abort(Abort),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Put {
    pub queue: String,
    pub job: Value,
    pub pri: u32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Get {
    pub queues: Vec<String>,
    pub wait: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Delete {
    pub id: u32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Abort {
    pub id: u32,
}

#[derive(Serialize, Debug)]
pub struct Response {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    job: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pri: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    queue: Option<String>,
}

impl Response {
    pub fn ok() -> Self {
        Self {
            status: "ok".to_string(),
            error: None,
            id: None,
            job: None,
            pri: None,
            queue: None,
        }
    }

    pub fn ok_and_id(id: u32) -> Self {
        Self {
            status: "ok".to_string(),
            error: None,
            id: Some(id),
            job: None,
            pri: None,
            queue: None,
        }
    }

    pub fn ok_and_job(job: Job) -> Self {
        Self {
            status: "ok".to_string(),
            error: None,
            id: Some(job.id),
            job: Some(job.job),
            pri: Some(job.prio),
            queue: Some(job.queue),
        }
    }

    pub fn no_job() -> Self {
        Self {
            status: "no-job".to_string(),
            error: None,
            id: None,
            job: None,
            pri: None,
            queue: None,
        }
    }

    pub fn error(error: &str) -> Self {
        Self {
            status: "error".to_string(),
            error: Some(error.to_string()),
            id: None,
            job: None,
            pri: None,
            queue: None,
        }
    }
}

impl Display for Response {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", to_string(self).unwrap())
    }
}

#[derive(Serialize, Debug)]
pub struct GetResponse {
    status: String,
    id: Option<u32>,
    job: Option<Value>,
    pri: Option<u32>,
    queue: Option<String>,
}

#[derive(Debug)]
pub enum Error {
    InvalidMsg,
    Serde(serde_json::Error),
    Io(io::Error),
    Utf8Error(FromUtf8Error),
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::Io(value)
    }
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Utf8Error(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Serde(value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn parse(str: &str) -> Result<Msg> {
    if let Value::Object(mut map) = from_str(str)? {
        match map.get("request") {
            Some(value) if *value == json!("put") => Ok(Msg::Put(from_value(Value::Object(map))?)),
            Some(value) if *value == json!("get") => {
                map.entry("wait").or_insert(json!(false));
                Ok(Msg::Get(from_value(Value::Object(map))?))
            }
            Some(value) if *value == json!("delete") => {
                Ok(Msg::Delete(from_value(Value::Object(map))?))
            }
            Some(value) if *value == json!("abort") => {
                Ok(Msg::Abort(from_value(Value::Object(map))?))
            }
            _ => Err(Error::InvalidMsg),
        }
    } else {
        Err(Error::InvalidMsg)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, Abort, Delete, Get, Msg, Put};
    use crate::jobcentre::msg::{Error, Response};

    #[test]
    fn parse_put_msg() {
        let put_msg = r#"
           {"request": "put", "queue": "q1", "job": {}, "pri": 10}
        "#;
        assert!(matches!(parse(put_msg), Ok(Msg::Put(_))));
    }

    #[test]
    fn parse_get_msg() {
        let get_msg = r#"
           {"request": "get", "queues": ["q1", "q2"]}
        "#;
        assert!(matches!(parse(get_msg), Ok(Msg::Get(_))));

        let get_msg_wait = r#"
           {"request": "get", "queues": ["q1", "q2"], "wait": true}
        "#;
        assert!(matches!(parse(get_msg_wait), Ok(Msg::Get(_))));
    }

    #[test]
    fn parse_delete_msg() {
        let delete_msg = r#"
           {"request": "delete", "id": 12345}
        "#;

        assert!(matches!(parse(delete_msg), Ok(Msg::Delete(_))));
    }

    #[test]
    fn parse_abort_msg() {
        let abort_msg = r#"
           {"request": "abort", "id": 12345}
        "#;

        assert!(matches!(parse(abort_msg), Ok(Msg::Abort(_))));
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
        assert!(matches!(parse(invalid_msg), Err(Error::InvalidMsg)));
    }
}
