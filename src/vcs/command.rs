use regex::Regex;
use std::fmt::{Display, Formatter};
use std::io;
use std::str::FromStr;
use std::string::FromUtf8Error;

pub enum Command {
    Get(Get),
    Put(Put),
    List(List),
    Help,
}

pub struct Get {
    pub filename: String,
    pub revision: Option<u32>,
}

impl Get {
    fn parse(words: &[&str]) -> Result<Self> {
        if words.is_empty() || words.len() > 2 {
            return Err(Error::Usage("GET file [revision]".to_string()));
        }
        let filename = words[0];
        let revnum = words
            .get(1)
            .map(|w| w.trim_start_matches('r'))
            .and_then(|w| u32::from_str(w).ok());
        if valid_file(filename) {
            Ok(Self {
                filename: filename.to_string(),
                revision: revnum,
            })
        } else {
            Err(Error::IllegalFileName)
        }
    }
}

pub struct Put {
    pub filename: String,
    pub length: usize,
}

impl Put {
    fn parse(words: &[&str]) -> Result<Self> {
        if words.len() != 2 {
            return Err(Error::Usage("PUT file length newline data".to_string()));
        }
        let filename = words[0];
        if valid_file(filename) {
            Ok(Self {
                filename: filename.to_string(),
                length: usize::from_str(words[1]).unwrap_or(0),
            })
        } else {
            Err(Error::IllegalFileName)
        }
    }
}

pub struct List {
    pub dir: String,
}

impl List {
    fn parse(words: &[&str]) -> Result<Self> {
        if words.len() != 1 {
            return Err(Error::Usage("LIST dir".to_string()));
        }
        let dir = words[0].to_string();
        if valid_path(&dir) {
            Ok(Self { dir })
        } else {
            Err(Error::IllegalDirName)
        }
    }
}

#[derive(Debug)]
pub enum Error {
    IllegalMethod(String),
    IllegalFileName,
    IllegalDirName,
    Usage(String),
    FromUtf8Error(FromUtf8Error),
    IoError(io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IllegalMethod(method) => write!(f, "ERR illegal method: {}", method),
            Error::Usage(usage) => write!(f, "ERR usage: {}", usage),
            Error::IllegalDirName => write!(f, "ERR illegal dir name"),
            Error::IllegalFileName => write!(f, "ERR illegal file name"),
            Error::FromUtf8Error(error) => write!(f, "ERR {}", error),
            Error::IoError(error) => write!(f, "ERR {}", error),
        }
    }
}

impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Error::FromUtf8Error(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::IoError(value)
    }
}

pub fn parse(buf: &[u8]) -> Result<Command> {
    let decoded_buf = String::from_utf8(buf.to_vec())?;
    let words: Vec<_> = decoded_buf.split_ascii_whitespace().collect();
    if words.is_empty() {
        return Err(Error::IllegalMethod("".to_string()));
    }
    match words[0] {
        "GET" => Ok(Command::Get(Get::parse(&words[1..])?)),
        "PUT" => Ok(Command::Put(Put::parse(&words[1..])?)),
        "LIST" => Ok(Command::List(List::parse(&words[1..])?)),
        "HELP" => Ok(Command::Help),
        illegal_method => Err(Error::IllegalMethod(illegal_method.to_string())),
    }
}

fn valid_path(path: &str) -> bool {
    let re = Regex::new(r"/{2,}").unwrap();
    path.starts_with('/')
        && path
            .chars()
            .all(|p| p.is_ascii_alphanumeric() || "/_-.".contains(p))
        && !re.is_match(path)
}

fn valid_file(path: &str) -> bool {
    valid_path(path) && !path.ends_with('/')
}

#[cfg(test)]
mod tests {
    use super::valid_path;

    #[test]
    fn valid_paths() {
        assert!(valid_path("/"));
        assert!(valid_path("/foo/bar/"));
        assert!(valid_path("/foo/bar/file1.txt"));
        assert!(valid_path("/foo/bar1/file"));
    }

    #[test]
    fn invalid_paths() {
        assert!(!valid_path("//"));
        assert!(!valid_path(r"/foo\/bar"));
        assert!(!valid_path("/foo//bar/file.txt"));
        assert!(!valid_path(r"/foo%/bar"));
        assert!(!valid_path("/foo/bär/"));
    }
}
