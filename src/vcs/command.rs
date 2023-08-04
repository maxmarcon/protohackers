use regex::Regex;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

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
        if valid_path(filename) {
            Ok(Self {
                filename: filename.to_string(),
                revision: words.get(1).and_then(|rev| u32::from_str(rev).ok()),
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
        if valid_path(filename) {
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
}

pub type Result<T> = std::result::Result<T, Error>;

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IllegalMethod(method) => write!(f, "ERR illegal method: {}", method),
            Error::Usage(usage) => write!(f, "ERR usage: {}", usage),
            Error::IllegalDirName => write!(f, "ERR illegal dir name"),
            Error::IllegalFileName => write!(f, "ERR illegal file name"),
        }
    }
}

pub fn parse(buf: &str) -> Result<Command> {
    let mut words: Vec<_> = buf.split_ascii_whitespace().rev().collect();
    if words.is_empty() {
        return Err(Error::IllegalMethod("".to_string()));
    }
    match words.pop().unwrap() {
        "GET" => Ok(Command::Get(Get::parse(&words[..])?)),
        "PUT" => Ok(Command::Put(Put::parse(&words[..])?)),
        "LIST" => Ok(Command::List(List::parse(&words[..])?)),
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
