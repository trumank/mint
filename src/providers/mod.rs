pub mod file;
pub mod http;
pub mod modio;

use crate::error::IntegrationError;
use crate::state::config::ConfigWrapper;

use anyhow::{Context, Result};

use serde::{Deserialize, Serialize};

use std::collections::{HashMap, HashSet};

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

type Providers = RwLock<HashMap<&'static str, Arc<dyn ModProvider>>>;
pub type ProviderCache = Arc<RwLock<ConfigWrapper<Cache>>>;

pub struct ModStore {
    providers: Providers,
    cache: ProviderCache,
    blob_cache: BlobCache,
}
impl ModStore {
    pub fn new<P: AsRef<Path>>(
        cache_path: P,
        parameters: &HashMap<String, HashMap<String, String>>,
    ) -> Result<Self> {
        let factories = inventory::iter::<ProviderFactory>()
            .map(|f| (f.id, f))
            .collect::<HashMap<_, _>>();
        let providers = parameters
            .iter()
            .flat_map(|(id, params)| {
                factories
                    .get(id.as_str())
                    .with_context(|| format!("unknown provider: {id}"))
                    .map(|f| (f.new)(params).map(|p| (f.id, p)))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        Ok(Self {
            providers: RwLock::new(providers),
            cache: Arc::new(RwLock::new(ConfigWrapper::new(
                cache_path.as_ref().join("cache.json"),
            ))),
            blob_cache: BlobCache::new(cache_path.as_ref().join("blobs")),
        })
    }
    pub fn add_provider(
        &self,
        provider_factory: &ProviderFactory,
        parameters: &HashMap<String, String>,
    ) -> Result<()> {
        let provider = (provider_factory.new)(parameters)?;
        self.providers
            .write()
            .unwrap()
            .insert(provider_factory.id, provider);
        Ok(())
    }
    pub fn get_provider(&self, url: &str) -> Result<Arc<dyn ModProvider>> {
        let factory = inventory::iter::<ProviderFactory>()
            .find(|f| (f.can_provide)(url))
            .with_context(|| format!("Could not find mod provider for {:?}", url))?;
        let lock = self.providers.read().unwrap();
        Ok(match lock.get(factory.id) {
            Some(e) => e.clone(),
            None => {
                return Err(IntegrationError::NoProvider {
                    url: url.to_string(),
                    factory,
                }
                .into())
            }
        })
    }
    pub async fn resolve_mods(
        &self,
        mods: &[ModSpecification],
        update: bool,
    ) -> Result<HashMap<ModSpecification, ModInfo>> {
        use futures::stream::{self, StreamExt, TryStreamExt};

        let mut to_resolve = mods.iter().cloned().collect::<HashSet<ModSpecification>>();
        let mut mods_map = HashMap::new();

        while !to_resolve.is_empty() {
            for (u, m) in stream::iter(
                to_resolve
                    .iter()
                    .map(|u| self.resolve_mod(u.to_owned(), update)),
            )
            .boxed()
            .buffered(5)
            .try_collect::<Vec<_>>()
            .await?
            {
                mods_map.insert(u, m);
                to_resolve.clear();
                for m in mods_map.values() {
                    for d in &m.suggested_dependencies {
                        if !mods_map.contains_key(d) {
                            to_resolve.insert(d.clone());
                        }
                    }
                }
            }
        }

        Ok(mods_map)
    }
    pub async fn resolve_mod(
        &self,
        original_spec: ModSpecification,
        update: bool,
    ) -> Result<(ModSpecification, ModInfo)> {
        let mut spec = original_spec.clone();
        loop {
            match self
                .get_provider(&spec.url)?
                .resolve_mod(&spec, update, self.cache.clone(), &self.blob_cache.clone())
                .await?
            {
                ModResponse::Resolve(m) => {
                    return Ok((original_spec, m));
                }
                ModResponse::Redirect(redirected_spec) => spec = redirected_spec,
            };
        }
    }
    pub async fn fetch_mods(&self, mods: &[&ModResolution], update: bool) -> Result<Vec<PathBuf>> {
        use futures::stream::{self, StreamExt, TryStreamExt};

        stream::iter(mods.iter().map(|res| self.fetch_mod(res, update)))
            .boxed() // without this the future becomes !Send https://github.com/rust-lang/rust/issues/104382
            .buffered(5)
            .try_collect::<Vec<_>>()
            .await
    }
    pub async fn fetch_mod(&self, res: &ModResolution, update: bool) -> Result<PathBuf> {
        self.get_provider(&res.url)?
            .fetch_mod(res, update, self.cache.clone(), &self.blob_cache.clone())
            .await
    }
    pub fn get_mod_info(&self, spec: &ModSpecification) -> Option<ModInfo> {
        self.get_provider(&spec.url)
            .ok()?
            .get_mod_info(spec, self.cache.clone())
    }
    pub fn is_pinned(&self, spec: &ModSpecification) -> bool {
        self.get_provider(&spec.url)
            .unwrap()
            .is_pinned(spec, self.cache.clone())
    }
    pub fn get_version_name(&self, spec: &ModSpecification) -> Option<String> {
        self.get_provider(&spec.url)
            .unwrap()
            .get_version_name(spec, self.cache.clone())
    }
}

pub trait ReadSeek: Read + Seek + Send {}
impl<T: Seek + Read + Send> ReadSeek for T {}

/// Whether a mod can be resolved by clients or not
#[derive(Debug, Clone)]
pub enum ResolvableStatus {
    /// If a mod can not be resolved, specify just a name
    Unresolvable { name: String },
    /// If a mod can be resolved, specify the URL
    Resolvable(ModResolution),
}

/// Returned from ModStore
#[derive(Debug, Clone)]
pub struct ModInfo {
    pub provider: &'static str,
    pub name: String,
    pub spec: ModSpecification,          // unpinned version
    pub versions: Vec<ModSpecification>, // pinned versions TODO make this a different type
    pub status: ResolvableStatus,
    pub suggested_require: bool,
    pub suggested_dependencies: Vec<ModSpecification>, // ModResponse
}

/// Returned from ModProvider
#[derive(Debug, Clone)]
pub enum ModResponse {
    Redirect(ModSpecification),
    Resolve(ModInfo),
}

/// Points to a mod, optionally a specific version
#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModSpecification {
    pub url: String,
}

/// Points to a specific version of a specific mod
#[derive(Debug, Clone)]
pub struct ModResolution {
    pub url: String,
}

#[async_trait::async_trait]
pub trait ModProvider: Send + Sync + std::fmt::Debug {
    async fn resolve_mod(
        &self,
        spec: &ModSpecification,
        update: bool,
        cache: ProviderCache,
        blob_cache: &BlobCache,
    ) -> Result<ModResponse>;
    async fn fetch_mod(
        &self,
        url: &ModResolution,
        update: bool,
        cache: ProviderCache,
        blob_cache: &BlobCache,
    ) -> Result<PathBuf>;
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
}

#[typetag::serde(tag = "type")]
pub trait ModProviderCache: Sync + Send + std::fmt::Debug {
    fn new() -> Self
    where
        Self: Sized;
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache(HashMap<String, Box<dyn ModProviderCache>>);
impl Cache {
    fn has<T: ModProviderCache + 'static>(&self, id: &str) -> bool {
        self.0
            .get(id)
            .and_then(|c| c.as_any().downcast_ref::<T>())
            .is_none()
    }
    fn get<T: ModProviderCache + 'static>(&self, id: &str) -> Option<&T> {
        self.0.get(id).and_then(|c| c.as_any().downcast_ref::<T>())
    }
    fn get_mut<T: ModProviderCache + 'static>(&mut self, id: &str) -> &mut T {
        if self.has::<T>(id) {
            self.0.insert(id.to_owned(), Box::new(T::new()));
        }
        self.0
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

        let tmp = self.path.join(format!(".{hash}"));
        std::fs::write(&tmp, blob)?;
        std::fs::rename(tmp, self.path.join(&hash))?;

        Ok(BlobRef(hash))
    }
    fn get_path(&self, blob: &BlobRef) -> Option<PathBuf> {
        let path = self.path.join(&blob.0);
        path.exists().then_some(path)
    }
}

inventory::collect!(ProviderFactory);
