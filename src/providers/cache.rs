use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use fs_err as fs;
use serde::{Deserialize, Serialize};
use tracing::*;

use mint_lib::mod_info::ModIdentifier;

use crate::state::config::ConfigWrapper;

pub type ProviderCache = Arc<RwLock<ConfigWrapper<VersionAnnotatedCache>>>;

#[typetag::serde(tag = "type")]
pub trait ModProviderCache: Sync + Send + std::fmt::Debug {
    fn new() -> Self
    where
        Self: Sized;
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

#[obake::versioned]
#[obake(version("0.0.0"))]
#[obake(version("0.1.0"))]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    #[obake(cfg(">=0.0.0"))]
    pub(super) cache: HashMap<String, Box<dyn ModProviderCache>>,
    #[obake(cfg(">=0.1.0"))]
    pub(super) gameplay_affecting_cache: HashMap<ModIdentifier, bool>,
}

impl Cache {
    pub(super) fn has<T: ModProviderCache + 'static>(&self, id: &str) -> bool {
        self.cache
            .get(id)
            .and_then(|c| c.as_any().downcast_ref::<T>())
            .is_none()
    }

    pub(super) fn get<T: ModProviderCache + 'static>(&self, id: &str) -> Option<&T> {
        self.cache
            .get(id)
            .and_then(|c| c.as_any().downcast_ref::<T>())
    }

    pub(super) fn get_mut<T: ModProviderCache + 'static>(&mut self, id: &str) -> &mut T {
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

pub(crate) fn read_cache_metadata_or_default(
    cache_metadata_path: &PathBuf,
) -> Result<VersionAnnotatedCache> {
    let cache: MaybeVersionedCache = match fs::read(cache_metadata_path) {
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

#[derive(Debug, Serialize, Deserialize)]
pub struct BlobRef(String);

#[derive(Debug, Clone)]
pub struct BlobCache {
    path: PathBuf,
}

impl BlobCache {
    pub(super) fn new<P: AsRef<Path>>(path: P) -> Self {
        fs::create_dir(&path).ok();
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub(super) fn write(&self, blob: &[u8]) -> Result<BlobRef> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(blob);
        let hash = hex::encode(hasher.finalize());

        let tmp = self.path.join(format!(".{hash}"));
        fs::write(&tmp, blob)?;
        fs::rename(tmp, self.path.join(&hash))?;

        Ok(BlobRef(hash))
    }

    pub(super) fn get_path(&self, blob: &BlobRef) -> Option<PathBuf> {
        let path = self.path.join(&blob.0);
        path.exists().then_some(path)
    }
}
