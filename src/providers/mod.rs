pub mod file;
pub mod http;
pub mod modio;

use crate::error::IntegrationError;

use anyhow::{anyhow, Result};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct ModStore {
    providers: HashMap<ProviderFactory, Box<dyn ModProvider>>,
    cache: Arc<RwLock<CacheWrapper>>,
    blob_cache: BlobCache,
}
impl ModStore {
    pub fn new<P: AsRef<Path>>(cache_path: P) -> Self {
        ModStore {
            providers: HashMap::new(),
            cache: Arc::new(RwLock::new(CacheWrapper::from_path(
                cache_path.as_ref().join("cache.json"),
            ))),
            blob_cache: BlobCache::new(cache_path.as_ref().join("blobs")),
        }
    }
    pub fn add_provider(&mut self, provider_factory: ProviderFactory) -> Result<()> {
        let provider = (provider_factory.new)()?;
        self.providers.insert(provider_factory, provider);
        Ok(())
    }
    pub fn get_provider(&self, url: &str) -> Result<&dyn ModProvider> {
        let factory = inventory::iter::<ProviderFactory>()
            .find(|f| (f.can_provide)(url.to_owned()))
            .ok_or_else(|| anyhow!("Could not find mod provider for {}", url))?;
        let entry = self.providers.get(factory);
        Ok(match entry {
            Some(e) => e.as_ref(),
            None => {
                return Err(IntegrationError::NoProvider {
                    url: url.to_owned(),
                    factory: factory.clone(),
                }
                .into())
            }
        })
    }
    pub async fn resolve_mods(&mut self, mods: &[String]) -> Result<Vec<Mod>> {
        use futures::stream::{self, StreamExt, TryStreamExt};

        stream::iter(mods.iter().map(|m| self.get_mod(m.to_owned())))
            .buffered(5)
            .try_collect()
            .await
    }
    pub async fn get_mod(&self, mut url: String) -> Result<Mod> {
        loop {
            match self
                .get_provider(&url)?
                .get_mod(&url, self.cache.clone(), &self.blob_cache.clone())
                .await?
            {
                ModResponse::Resolve { data, status } => {
                    return Ok(Mod { status, data });
                }
                ModResponse::Redirect {
                    url: redirected_url,
                } => url = redirected_url,
            };
        }
    }
}

pub trait ReadSeek: Read + Seek + Send {}
impl<T: Seek + Read + Send> ReadSeek for T {}

/// Whether a mod can be resolved by clients or not
#[derive(Debug)]
pub enum ResolvableStatus {
    /// If a mod can not be resolved, specify just a name
    Unresolvable { name: String },
    /// Ifa mod can be resolved, specify the URL
    Resolvable { url: String },
}

/// Returned from ModStore
pub struct Mod {
    pub status: ResolvableStatus,
    pub data: Box<dyn ReadSeek>,
}

/// Returned from ModProvider
pub enum ModResponse {
    Redirect {
        url: String,
    },
    Resolve {
        status: ResolvableStatus,
        data: Box<dyn ReadSeek>,
    },
}

#[async_trait::async_trait]
pub trait ModProvider: Sync + std::fmt::Debug {
    async fn get_mod(
        &self,
        url: &str,
        cache: Arc<RwLock<CacheWrapper>>,
        blob_cache: &BlobCache,
    ) -> Result<ModResponse>;
}

#[derive(Debug, Clone, Eq, Ord, Hash, PartialEq, PartialOrd)]
pub struct ProviderFactory {
    id: &'static str,
    new: fn() -> Result<Box<dyn ModProvider>>,
    can_provide: fn(String) -> bool,
}

#[typetag::serde(tag = "type")]
pub trait ModProviderCache: Sync + Send + std::fmt::Debug {
    fn new() -> Self
    where
        Self: Sized;
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

type Cache = HashMap<String, Box<dyn ModProviderCache>>;
#[derive(Debug, Default)]
pub struct CacheWrapper {
    path: PathBuf,
    cache: HashMap<String, Box<dyn ModProviderCache>>,
}
impl Drop for CacheWrapper {
    fn drop(&mut self) {
        self.write().ok();
    }
}
impl CacheWrapper {
    fn from_path<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            cache: std::fs::read(path)
                .ok()
                .and_then(|d| serde_json::from_slice::<Cache>(&d).ok())
                .unwrap_or_default(),
        }
    }
    fn write(&self) -> Result<()> {
        std::fs::write(&self.path, serde_json::to_string(&self.cache)?.as_bytes())?;
        Ok(())
    }
    fn has<T: ModProviderCache + 'static>(&self, id: &str) -> bool {
        self.cache
            .get(id)
            .and_then(|c| c.as_any().downcast_ref::<T>())
            .is_none()
    }
    fn get<T: ModProviderCache + 'static>(&mut self, id: &str) -> &T {
        if self.has::<T>(id) {
            self.cache.insert(id.to_owned(), Box::new(T::new()));
        }
        self.cache
            .get(id)
            .and_then(|c| c.as_any().downcast_ref::<T>())
            .unwrap()
    }
    fn get_mut<T: ModProviderCache + 'static>(&mut self, id: &str) -> &mut T {
        if self.has::<T>(id) {
            self.cache.insert(id.to_owned(), Box::new(T::new()));
        }
        self.cache
            .get_mut(id)
            .and_then(|c| c.as_any_mut().downcast_mut::<T>())
            .unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlobRef(String);

#[derive(Debug, Clone)]
pub struct BlobCache {
    path: PathBuf,
}
impl BlobCache {
    fn new<P: AsRef<Path>>(path: P) -> Self {
        std::fs::create_dir(&path).ok();
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
    fn write(&self, blob: &[u8]) -> Result<BlobRef> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(blob);
        let hash = hex::encode(hasher.finalize());

        std::fs::write(self.path.join(&hash), blob)?;

        Ok(BlobRef(hash))
    }
    fn read(&self, blob: &BlobRef) -> Result<Box<dyn ReadSeek>> {
        // TODO verify hash, custom reader that hashes as it's read?
        Ok(Box::new(BufReader::new(File::open(
            self.path.join(&blob.0),
        )?)))
    }
}

inventory::collect!(ProviderFactory);
