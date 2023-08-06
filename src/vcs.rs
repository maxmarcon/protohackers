use md5::Digest;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::DirBuilder;
use std::io;
use std::io::{Error, ErrorKind};
use std::path::{Component, Path};
use tokio::fs;
use tokio::io::AsyncWriteExt;

pub mod command;

static WORKING_DIR: &str = "vcs";

pub fn create_working_dir() -> io::Result<()> {
    DirBuilder::new().recursive(true).create(WORKING_DIR)
}

#[derive(Clone)]
pub struct File {
    name: String,
    revision_map: HashMap<u32, Digest>,
    current_revision: u32,
}

impl File {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            revision_map: HashMap::new(),
            current_revision: 0,
        }
    }

    pub  fn add_revision(&mut self, hash: Digest) -> bool {
        if self
            .revision_map
            .get(&self.current_revision)
            .is_some_and(|revision_hash| *revision_hash == hash)
        { 
            false
        } else {
            self.current_revision += 1;
            self.revision_map.insert(self.current_revision, hash);
            true
        }
    }

    pub async fn open(&self, rev: Option<u32>) -> io::Result<fs::File> {
        let hash = format!(
            "{:x}",
            self.revision_map[rev.as_ref().unwrap_or(&self.current_revision)]
        );
        fs::File::open(Path::join(WORKING_DIR.as_ref(), hash)).await
    }
}

#[derive(Default, Clone)]
pub struct Dir {
    files: HashMap<String, File>,
    dirs: HashMap<String, Dir>,
}

impl Dir {
    pub fn find_dir(&self, path: &str) -> Option<&Dir> {
        let mut cur_dir = self;
        let components: Vec<_> = Path::new(path).components().collect();
        for component in components {
            if let Component::Normal(component) = component {
                if let Some(next_dir) = cur_dir.dirs.get(&component.to_str().unwrap().to_string()) {
                    cur_dir = next_dir;
                } else {
                    return None;
                }
            }
        }
        Some(cur_dir)
    }

    pub fn find_file(&self, path: &str) -> Option<&File> {
        let mut cur_dir = self;
        let components: Vec<_> = Path::new(path).components().collect();
        let components_len = components.len();
        for (pos, component) in components.into_iter().enumerate() {
            match component {
                Component::Normal(component) if pos == components_len - 1 => {
                    return cur_dir.files.get(&component.to_str().unwrap().to_string())
                }
                Component::Normal(component) => {
                    if let Some(next_dir) =
                        cur_dir.dirs.get(&component.to_str().unwrap().to_string())
                    {
                        cur_dir = next_dir;
                    } else {
                        return None;
                    }
                }
                _ => (),
            }
        }
        None
    }

    pub  fn add_revision(&mut self, path: &str, hash: Digest) -> io::Result<(u32, bool)> {
        let mut cur_dir = self;
        let components: Vec<_> = Path::new(path).components().collect();
        let components_length = components.len();
        for (idx, component) in components.into_iter().enumerate() {
            match component {
                Component::Normal(component) if idx == components_length - 1 => {
                    let component = component.to_str().unwrap().to_string();
                    let file = cur_dir
                        .files
                        .entry(component.clone())
                        .or_insert(File::new(&component));
                    let new_revision = file.add_revision(hash);
                    return Ok((file.current_revision, new_revision));
                }
                Component::Normal(component) => {
                    cur_dir = cur_dir
                        .dirs
                        .entry(component.to_str().unwrap().to_string())
                        .or_insert(Dir::default())
                }
                _ => (),
            }
        }
        Err(Error::new(ErrorKind::Other, "BAM!"))
    }
}

impl Display for Dir {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let total = self
            .dirs
            .keys()
            .filter(|dir| !self.files.contains_key(*dir))
            .count();
        writeln!(f, "OK {}", total)?;
        for dir in self
            .dirs
            .keys()
            .filter(|dir| !self.files.contains_key(*dir))
        {
            writeln!(f, "{}/ DIR", dir)?;
        }
        for (file_name, file) in self.files.iter() {
            writeln!(f, "{} r{}", file_name, file.current_revision)?;
        }
        Ok(())
    }
}
