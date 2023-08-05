use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc::Sender;

use super::{
    BlobCache, FetchProgress, ModInfo, ModProvider, ModResolution, ModResponse, ModSpecification,
    ProviderCache,
};

inventory::submit! {
    super::ProviderFactory {
        id: FILE_PROVIDER_ID,
        new: FileProvider::new_provider,
        can_provide: |url| Path::new(url).exists(),
        parameters: &[],
    }
}

#[derive(Debug)]
pub struct FileProvider {}

impl FileProvider {
    pub fn new_provider(_parameters: &HashMap<String, String>) -> Result<Arc<dyn ModProvider>> {
        Ok(Arc::new(Self::new()))
    }
    pub fn new() -> Self {
        Self {}
    }
}

const FILE_PROVIDER_ID: &str = "file";

#[async_trait::async_trait]
impl ModProvider for FileProvider {
    async fn resolve_mod(
        &self,
        spec: &ModSpecification,
        _update: bool,
        _cache: ProviderCache,
        _blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let path = Path::new(&spec.url);
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| spec.url.to_string());
        Ok(ModResponse::Resolve(ModInfo {
            provider: FILE_PROVIDER_ID,
            name,
            spec: spec.clone(),
            versions: vec![],
            resolution: ModResolution::unresolvable(path.to_string_lossy().to_string()),
            suggested_require: false,
            suggested_dependencies: vec![],
            modio_tags: None,
            modio_id: None,
        }))
    }

    async fn fetch_mod(
        &self,
        res: &ModResolution,
        _update: bool,
        _cache: ProviderCache,
        _blob_cache: &BlobCache,
        tx: Option<Sender<FetchProgress>>,
    ) -> Result<PathBuf> {
        if let Some(tx) = tx {
            tx.send(FetchProgress::Complete {
                resolution: res.clone(),
            })
            .await
            .unwrap();
        }
        Ok(PathBuf::from(&res.url))
    }

    async fn check(&self) -> Result<()> {
        Ok(())
    }

    fn get_mod_info(&self, spec: &ModSpecification, _cache: ProviderCache) -> Option<ModInfo> {
        let path = Path::new(&spec.url);
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| spec.url.to_string());
        Some(ModInfo {
            provider: FILE_PROVIDER_ID,
            name,
            spec: spec.clone(),
            versions: vec![],
            resolution: ModResolution::unresolvable(path.to_string_lossy().to_string()),
            suggested_require: false,
            suggested_dependencies: vec![],
            modio_tags: None,
            modio_id: None,
        })
    }

    fn is_pinned(&self, _spec: &ModSpecification, _cache: ProviderCache) -> bool {
        true
    }
    fn get_version_name(&self, _spec: &ModSpecification, _cache: ProviderCache) -> Option<String> {
        Some("latest".to_string())
    }
}
