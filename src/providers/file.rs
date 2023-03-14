use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};

use super::{BlobCache, CacheWrapper, ModProvider, ModResponse, ResolvableStatus};

inventory::submit! {
    super::ProviderFactory {
        id: "file",
        new: FileProvider::new_provider,
        can_provide: |url| Path::new(&url).exists()
    }
}

#[derive(Debug)]
pub struct FileProvider {}

impl FileProvider {
    pub fn new_provider() -> Result<Box<dyn ModProvider>> {
        Ok(Box::new(Self::new()))
    }
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl ModProvider for FileProvider {
    async fn get_mod(
        &self,
        url: &str,
        _update: bool,
        _cache: Arc<RwLock<CacheWrapper>>,
        _blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let path = Path::new(url);
        Ok(ModResponse::Resolve {
            status: ResolvableStatus::Unresolvable {
                name: path
                    .file_name()
                    .ok_or_else(|| anyhow!("could not determine file name of {}", url))?
                    .to_string_lossy()
                    .to_string(),
            },
            data: Box::new(BufReader::new(File::open(path)?)),
        })
    }
}
