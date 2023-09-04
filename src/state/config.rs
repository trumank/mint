use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use serde::{de::DeserializeOwned, Serialize};

pub trait ConfigTrait: std::fmt::Debug + Default + Serialize + DeserializeOwned {}
impl<T> ConfigTrait for T where T: std::fmt::Debug + Default + Serialize + DeserializeOwned {}

/// Wrapper around an object that is written to a file when dropped
#[derive(Debug)]
pub struct ConfigWrapper<C: ConfigTrait> {
    path: Option<PathBuf>,
    config: C,
}

impl<C: ConfigTrait> ConfigWrapper<C> {
    /// Creates a new file backed config object that is automatically saved when dropped
    pub fn new<P: AsRef<Path>>(path: P, config: C) -> Self {
        Self {
            config,
            path: Some(path.as_ref().to_path_buf()),
        }
    }

    /// Create wrapper that lives only in memory and has no file backing
    pub fn memory(config: C) -> Self {
        Self { config, path: None }
    }

    /// Try our best to ensure that the config written is complete to protect against partial
    /// or broken config writes if the tool crashes or is killed.
    ///
    /// This is achieved, best-effort, by writing to a temporary file then replacing the target file
    /// with the temporary file.
    ///
    /// See <https://stackoverflow.com/questions/70362352/atomic-file-create-write>.
    pub fn save(&self) -> Result<()> {
        if let Some(final_path) = &self.path {
            let mut temp_file = tempfile::NamedTempFile::new_in(final_path.parent().unwrap())?;
            temp_file
                .write_all(&serde_json::to_vec_pretty(&self.config)?)
                .context("failed to write to tempfile")?;
            temp_file
                .persist(final_path)
                .context("failed to replace destination file with tempfile")?;
        }
        Ok(())
    }
}

impl<C: ConfigTrait> std::ops::Deref for ConfigWrapper<C> {
    type Target = C;
    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl<C: ConfigTrait> std::ops::DerefMut for ConfigWrapper<C> {
    fn deref_mut(&mut self) -> &mut C {
        &mut self.config
    }
}

impl<C: ConfigTrait> Drop for ConfigWrapper<C> {
    fn drop(&mut self) {
        self.save().unwrap();
    }
}
