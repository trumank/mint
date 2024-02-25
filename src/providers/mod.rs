pub mod file;
pub mod http;
pub mod modio;
#[macro_use]
pub mod cache;
pub mod mod_store;

use anyhow::Result;
use tokio::sync::mpsc::Sender;

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub use cache::*;
pub use mint_lib::mod_info::*;
pub use mod_store::*;

type Providers = RwLock<HashMap<&'static str, Arc<dyn ModProvider>>>;

pub trait ReadSeek: Read + Seek + Send {}
impl<T: Seek + Read + Send> ReadSeek for T {}

#[derive(Debug)]
pub enum FetchProgress {
    Progress {
        resolution: ModResolution,
        progress: u64,
        size: u64,
    },
    Complete {
        resolution: ModResolution,
    },
}

impl FetchProgress {
    pub fn resolution(&self) -> &ModResolution {
        match self {
            FetchProgress::Progress { resolution, .. } => resolution,
            FetchProgress::Complete { resolution, .. } => resolution,
        }
    }
}

#[async_trait::async_trait]
pub trait ModProvider: Send + Sync {
    async fn resolve_mod(
        &self,
        spec: &ModSpecification,
        update: bool,
        cache: ProviderCache,
    ) -> Result<ModResponse>;
    async fn fetch_mod(
        &self,
        url: &ModResolution,
        update: bool,
        cache: ProviderCache,
        blob_cache: &BlobCache,
        tx: Option<Sender<FetchProgress>>,
    ) -> Result<PathBuf>;
    async fn update_cache(&self, cache: ProviderCache) -> Result<()>;
    /// Check if provider is configured correctly
    async fn check(&self) -> Result<()>;
    fn get_mod_info(&self, spec: &ModSpecification, cache: ProviderCache) -> Option<ModInfo>;
    fn is_pinned(&self, spec: &ModSpecification, cache: ProviderCache) -> bool;
    fn get_version_name(&self, spec: &ModSpecification, cache: ProviderCache) -> Option<String>;
}

#[derive(Clone)]
pub struct ProviderFactory {
    pub id: &'static str,
    #[allow(clippy::type_complexity)]
    new: fn(&HashMap<String, String>) -> Result<Arc<dyn ModProvider>>,
    can_provide: fn(&str) -> bool,
    pub parameters: &'static [ProviderParameter<'static>],
}

impl std::fmt::Debug for ProviderFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderFactory")
            .field("id", &self.id)
            .field("parameters", &self.parameters)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct ProviderParameter<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub link: Option<&'a str>,
}

inventory::collect!(ProviderFactory);
