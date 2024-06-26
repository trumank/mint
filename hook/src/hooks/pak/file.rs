use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;

use super::{FileInfo, FileProvider};

#[derive(Debug, Deserialize)]
pub struct PlainFileProviderConfig {
    path: PathBuf,
    globs: Vec<String>,
}
impl PlainFileProviderConfig {
    pub fn build(self) -> Result<PlainFileProvider> {
        PlainFileProvider::new(self)
    }
}

pub struct PlainFileProvider {
    path: PathBuf,
    globs: Vec<glob::Pattern>,
}
impl PlainFileProvider {
    pub fn new(config: PlainFileProviderConfig) -> Result<Self> {
        Ok(Self {
            path: config.path,
            globs: config
                .globs
                .iter()
                .flat_map(|g| glob::Pattern::new(g))
                .collect(),
        })
    }
}

impl FileProvider for PlainFileProvider {
    fn matches(&self, path: &str) -> bool {
        self.globs.iter().any(|g| g.matches(path))
    }
    fn get_file_info(&mut self, path: &str) -> Result<FileInfo> {
        match std::fs::File::open(self.path.join(path)) {
            Ok(file) => {
                let meta = file.metadata()?;
                Ok(FileInfo {
                    file_exists: true,
                    read_only: meta.permissions().readonly(),
                    size: meta.len() as i64,
                    timestamp: 0,        // TODO timestamp
                    access_timestamp: 0, // TODO timestamp
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileInfo {
                file_exists: false,
                ..Default::default()
            }),
            Err(e) => Err(e.into()),
        }
    }
    fn get_file(&mut self, path: &str) -> Result<Vec<u8>> {
        Ok(std::fs::read(self.path.join(path))?)
    }
}
