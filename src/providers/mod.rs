pub mod file;
pub mod http;
pub mod modio;

use crate::error::IntegrationError;
use crate::state::config::ConfigWrapper;
use crate::write_file;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;
use tracing::{debug, info};

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub use mint_lib::mod_info::*;

type Providers = RwLock<HashMap<&'static str, Arc<dyn ModProvider>>>;

pub type ProviderCache = Arc<RwLock<ConfigWrapper<VersionAnnotatedCache>>>;

#[obake::versioned]
#[obake(version("0.0.0"))]
#[obake(version("0.1.0"))]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    #[obake(cfg(">=0.0.0"))]
    cache: HashMap<String, Box<dyn ModProviderCache>>,
    #[obake(cfg(">=0.1.0"))]
    gameplay_affecting_cache: HashMap<ModIdentifier, bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum VersionAnnotatedCache {
    #[serde(rename = "0.0.0")]
    V0_0_0(Cache!["0.0.0"]),
    #[serde(rename = "0.1.0")]
    V0_1_0(Cache!["0.1.0"]),
}

impl From<Cache!["0.0.0"]> for Cache!["0.1.0"] {
    fn from(legacy: Cache!["0.0.0"]) -> Self {
        Self {
            cache: legacy.cache,
            gameplay_affecting_cache: Default::default(),
        }
    }
}

impl Default for VersionAnnotatedCache {
    fn default() -> Self {
        VersionAnnotatedCache::V0_1_0(Default::default())
    }
}

impl Deref for VersionAnnotatedCache {
    type Target = Cache!["0.1.0"];

    fn deref(&self) -> &Self::Target {
        match self {
            VersionAnnotatedCache::V0_0_0(_) => unreachable!(),
            VersionAnnotatedCache::V0_1_0(c) => c,
        }
    }
}

impl DerefMut for VersionAnnotatedCache {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            VersionAnnotatedCache::V0_0_0(_) => unreachable!(),
            VersionAnnotatedCache::V0_1_0(c) => c,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MaybeVersionedCache {
    Versioned(VersionAnnotatedCache),
    Legacy(Cache!["0.0.0"]),
}

impl Default for MaybeVersionedCache {
    fn default() -> Self {
        MaybeVersionedCache::Versioned(Default::default())
    }
}

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
        let providers = inventory::iter::<ProviderFactory>()
            .flat_map(|f| {
                let params = parameters.get(f.id).cloned().unwrap_or_default();
                f.parameters
                    .iter()
                    .all(|p| params.contains_key(p.id))
                    .then(|| ((f.new)(&params).map(|p| (f.id, p))))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        let cache_metadata_path = cache_path.as_ref().join("cache.json");

        let cache = read_cache_metadata_or_default(&cache_metadata_path)?;
        let cache = ConfigWrapper::new(&cache_metadata_path, cache);
        cache.save().unwrap();

        Ok(Self {
            providers: RwLock::new(providers),
            cache: Arc::new(RwLock::new(cache)),
            blob_cache: BlobCache::new(cache_path.as_ref().join("blobs")),
        })
    }

    pub fn get_provider_factories() -> impl Iterator<Item = &'static ProviderFactory> {
        inventory::iter::<ProviderFactory>()
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

    pub async fn add_provider_checked(
        &self,
        provider_factory: &ProviderFactory,
        parameters: &HashMap<String, String>,
    ) -> Result<()> {
        let provider = (provider_factory.new)(parameters)?;
        provider.check().await?;
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

        // used to deduplicate dependencies from mods already present in the mod list
        let mut precise_mod_specs = HashSet::new();

        while !to_resolve.is_empty() {
            for (u, m) in stream::iter(
                to_resolve
                    .iter()
                    .map(|u| self.resolve_mod(u.to_owned(), update)),
            )
            .boxed()
            .buffer_unordered(5)
            .try_collect::<Vec<_>>()
            .await?
            {
                precise_mod_specs.insert(m.spec.clone());
                mods_map.insert(u, m);
                to_resolve.clear();
                for m in mods_map.values() {
                    for d in &m.suggested_dependencies {
                        if !precise_mod_specs.contains(d) {
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
                .resolve_mod(&spec, update, self.cache.clone())
                .await?
            {
                ModResponse::Resolve(m) => {
                    return Ok((original_spec, m));
                }
                ModResponse::Redirect(redirected_spec) => spec = redirected_spec,
            };
        }
    }

    pub async fn fetch_mods(
        &self,
        mods: &[ModResolution],
        update: bool,
        tx: Option<Sender<FetchProgress>>,
    ) -> Result<Vec<PathBuf>> {
        use futures::stream::{self, StreamExt, TryStreamExt};

        stream::iter(
            mods.iter()
                .map(|res| self.fetch_mod(res, update, tx.clone())),
        )
        .boxed() // without this the future becomes !Send https://github.com/rust-lang/rust/issues/104382
        .buffer_unordered(5)
        .try_collect::<Vec<_>>()
        .await
    }

    pub async fn fetch_mods_ordered(
        &self,
        mods: &[&ModResolution],
        update: bool,
        tx: Option<Sender<FetchProgress>>,
    ) -> Result<Vec<PathBuf>> {
        use futures::stream::{self, StreamExt, TryStreamExt};

        stream::iter(
            mods.iter()
                .map(|res| self.fetch_mod(res, update, tx.clone())),
        )
        .boxed() // without this the future becomes !Send https://github.com/rust-lang/rust/issues/104382
        .buffered(5)
        .try_collect::<Vec<_>>()
        .await
    }

    pub async fn fetch_mod(
        &self,
        res: &ModResolution,
        update: bool,
        tx: Option<Sender<FetchProgress>>,
    ) -> Result<PathBuf> {
        self.get_provider(&res.url.0)?
            .fetch_mod(
                res,
                update,
                self.cache.clone(),
                &self.blob_cache.clone(),
                tx,
            )
            .await
    }

    pub async fn update_cache(&self) -> Result<()> {
        let providers = self.providers.read().unwrap().clone();
        for (name, provider) in providers.iter() {
            info!("updating cache for {name} provider");
            provider.update_cache(self.cache.clone()).await?;
        }
        Ok(())
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

    pub fn update_gameplay_affecting_status(&self, id: ModIdentifier, stat: bool) {
        self.cache
            .write()
            .unwrap()
            .gameplay_affecting_cache
            .insert(id, stat);
    }

    pub fn get_gameplay_affecting_status(&self, id: &ModIdentifier) -> Option<bool> {
        self.cache
            .read()
            .unwrap()
            .gameplay_affecting_cache
            .get(id)
            .copied()
    }
}

fn read_cache_metadata_or_default(cache_metadata_path: &PathBuf) -> Result<VersionAnnotatedCache> {
    let cache: MaybeVersionedCache = match std::fs::read(cache_metadata_path) {
        Ok(buf) => {
            let mut dyn_value = serde_json::from_slice::<serde_json::Value>(&buf)
                .context("failed to deserialize cache metadata into dynamic json value")?;
            let obj_map = dyn_value
                .as_object_mut()
                .context("failed to deserialize cache metadata into object map")?;
            let version = obj_map.remove("version");
            debug!(?version);
            if let Some(v) = version
                && let serde_json::Value::String(vs) = v
            {
                match vs.as_str() {
                    "0.0.0" => {
                        // HACK: workaround a serde issue relating to flattening with tags
                        // involving numeric keys in hashmaps, see
                        // <https://github.com/serde-rs/serde/issues/1183>.
                        match serde_json::from_slice::<Cache!["0.0.0"]>(&buf) {
                            Ok(c) => {
                                debug!("read as cache version v0.0.0");
                                MaybeVersionedCache::Versioned(VersionAnnotatedCache::V0_0_0(c))
                            }
                            Err(e) => Err(e).context("failed to deserialize cache as v0.0.0")?,
                        }
                    }
                    "0.1.0" => {
                        // HACK: workaround a serde issue relating to flattening with tags
                        // involving numeric keys in hashmaps, see
                        // <https://github.com/serde-rs/serde/issues/1183>.
                        match serde_json::from_slice::<Cache!["0.1.0"]>(&buf) {
                            Ok(c) => {
                                debug!("read as cache version v0.1.0");
                                MaybeVersionedCache::Versioned(VersionAnnotatedCache::V0_1_0(c))
                            }
                            Err(e) => Err(e).context("failed to deserialize cache as v0.1.0")?,
                        }
                    }
                    _ => unimplemented!(),
                }
            } else {
                // HACK: workaround a serde issue relating to flattening with tags involving
                // numeric keys in hashmaps, see <https://github.com/serde-rs/serde/issues/1183>.
                match serde_json::from_slice::<HashMap<String, Box<dyn ModProviderCache>>>(&buf) {
                    Ok(c) => MaybeVersionedCache::Legacy(Cache_v0_0_0 { cache: c }),
                    Err(e) => Err(e).context("failed to deserialize legacy cache")?,
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => MaybeVersionedCache::default(),
        Err(e) => Err(e).context("failed to read `cache.json`")?,
    };

    let cache: VersionAnnotatedCache = match cache {
        MaybeVersionedCache::Versioned(v) => match v {
            VersionAnnotatedCache::V0_0_0(v) => VersionAnnotatedCache::V0_1_0(v.into()),
            VersionAnnotatedCache::V0_1_0(v) => VersionAnnotatedCache::V0_1_0(v),
        },
        MaybeVersionedCache::Legacy(legacy) => VersionAnnotatedCache::V0_0_0(legacy),
    };

    Ok(cache)
}

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

#[typetag::serde(tag = "type")]
pub trait ModProviderCache: Sync + Send + std::fmt::Debug {
    fn new() -> Self
    where
        Self: Sized;
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl Cache {
    fn has<T: ModProviderCache + 'static>(&self, id: &str) -> bool {
        self.cache
            .get(id)
            .and_then(|c| c.as_any().downcast_ref::<T>())
            .is_none()
    }

    fn get<T: ModProviderCache + 'static>(&self, id: &str) -> Option<&T> {
        self.cache
            .get(id)
            .and_then(|c| c.as_any().downcast_ref::<T>())
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

        let tmp = self.path.join(format!(".{hash}"));
        write_file(&tmp, blob)?;
        std::fs::rename(tmp, self.path.join(&hash))?;

        Ok(BlobRef(hash))
    }

    fn get_path(&self, blob: &BlobRef) -> Option<PathBuf> {
        let path = self.path.join(&blob.0);
        path.exists().then_some(path)
    }
}

inventory::collect!(ProviderFactory);
