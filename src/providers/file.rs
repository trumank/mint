use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};

use super::{BlobCache, Cache, Mod, ModProvider, ModResponse, ModSpecification, ResolvableStatus};
use crate::config::ConfigWrapper;

inventory::submit! {
    super::ProviderFactory {
        id: "file",
        new: FileProvider::new_provider,
        can_provide: |spec| Path::new(&spec.url).exists(),
        parameters: &[],
    }
}

#[derive(Debug)]
pub struct FileProvider {}

impl FileProvider {
    pub fn new_provider(_parameters: &HashMap<String, String>) -> Result<Box<dyn ModProvider>> {
        Ok(Box::new(Self::new()))
    }
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl ModProvider for FileProvider {
    async fn resolve_mod(
        &self,
        spec: &ModSpecification,
        _update: bool,
        _cache: Arc<RwLock<ConfigWrapper<Cache>>>,
        _blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let path = Path::new(&spec.url);
        Ok(ModResponse::Resolve(Mod {
            spec: spec.clone(),
            status: ResolvableStatus::Unresolvable {
                name: path
                    .file_name()
                    .ok_or_else(|| anyhow!("could not determine file name of {:?}", spec))?
                    .to_string_lossy()
                    .to_string(),
            },
            suggested_require: false,
            suggested_dependencies: vec![],
        }))
    }

    async fn fetch_mod(
        &self,
        url: &str,
        _update: bool,
        _cache: Arc<RwLock<ConfigWrapper<Cache>>>,
        _blob_cache: &BlobCache,
    ) -> Result<PathBuf> {
        Ok(PathBuf::from(url))
    }
}
