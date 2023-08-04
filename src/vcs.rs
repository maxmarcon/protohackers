use std::collections::HashMap;
use std::io::Write;

pub mod command;

pub struct File {
    name: String,
    revision_map: HashMap<u32, String>,
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

    pub fn add_revision(&mut self, buf: &[u8]) -> std::io::Result<bool> {
        let hash = format!("{:x}", md5::compute(buf));
        if self.revision_map.get(&self.current_revision).is_some_and(|revision_hash| *revision_hash == hash) {
            Ok(false)
        } else {
            let mut file = std::fs::File::open(&hash)?;
            file.write_all(buf)?;
            self.current_revision += 1;
            self.revision_map.insert(self.current_revision, hash);
            Ok(true)
        }
    }
}

pub struct Dir {
    files: HashMap<String, File>,
    dirs: HashMap<String, Dir>,
}

