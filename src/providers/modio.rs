use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};
use serde::{Deserialize, Serialize};
use task_local_extensions::Extensions;

use super::{
    BlobCache, BlobRef, Cache, Mod, ModProvider, ModProviderCache, ModResolution, ModResponse,
    ResolvableStatus,
};
use crate::config::ConfigWrapper;

lazy_static::lazy_static! {
    static ref RE_MOD: regex::Regex = regex::Regex::new("^https://mod.io/g/drg/m/(?P<name_id>[^/#]+)(:?#(?P<mod_id>\\d+)(:?/(?P<modfile_id>\\d+))?)?$").unwrap();
}

const MODIO_DRG_ID: u32 = 2475;
const MODIO_PROVIDER_ID: &str = "modio";

inventory::submit! {
    super::ProviderFactory {
        id: MODIO_PROVIDER_ID,
        new: ModioProvider::new_provider,
        can_provide: |url| RE_MOD.is_match(&url),
        parameters: &[
            super::ProviderParameter {
                id: "oauth",
                name: "OAuth Token",
                description: "mod.io OAuth token. Obtain from https://mod.io/me/access",
            },
        ]
    }
}

#[derive(Debug)]
pub struct ModioProvider {
    modio: modio::Modio,
}

impl ModioProvider {
    fn new_provider(parameters: &HashMap<String, String>) -> Result<Box<dyn ModProvider>> {
        let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with::<LoggingMiddleware>(Default::default())
            .build();
        let modio = modio::Modio::new(
            modio::Credentials::with_token(
                "".to_owned(), // TODO patch modio to not use API key at all
                parameters
                    .get("oauth")
                    .ok_or_else(|| anyhow!("missing OAuth token param"))?,
            ),
            client,
        )?;

        Ok(Box::new(Self::new(modio)))
    }
    fn new(modio: modio::Modio) -> Self {
        Self { modio }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModioCache {
    mod_id_map: HashMap<String, u32>,
    modfile_blobs: HashMap<u32, BlobRef>,
    dependencies: HashMap<u32, Vec<u32>>,
    mods: HashMap<u32, ModioMod>,
}

#[typetag::serde]
impl ModProviderCache for ModioCache {
    fn new() -> Self {
        Default::default()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModioMod {
    name_id: String,
    latest_modfile: Option<u32>,
    tags: HashSet<String>,
}
impl From<modio::mods::Mod> for ModioMod {
    fn from(value: modio::mods::Mod) -> Self {
        Self {
            name_id: value.name_id,
            latest_modfile: value.modfile.map(|f| f.id),
            tags: value.tags.into_iter().map(|t| t.name).collect(),
        }
    }
}

#[derive(Default)]
struct LoggingMiddleware {
    requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl Middleware for LoggingMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        loop {
            println!(
                "Request started {} {:?}",
                self.requests
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                req.url().path()
            );
            let res = next.clone().run(req.try_clone().unwrap(), extensions).await;
            if let Ok(res) = &res {
                if let Some(retry) = res.headers().get("retry-after") {
                    println!("retrying after: {}...", retry.to_str().unwrap());
                    tokio::time::sleep(tokio::time::Duration::from_secs(
                        retry.to_str().unwrap().parse::<u64>().unwrap(),
                    ))
                    .await;
                    continue;
                }
            }
            return res;
        }
    }
}

#[async_trait::async_trait]
impl ModProvider for ModioProvider {
    async fn resolve_mod(
        &self,
        url: &str,
        update: bool,
        cache: Arc<RwLock<ConfigWrapper<Cache>>>,
        _blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let captures = RE_MOD
            .captures(url)
            .ok_or_else(|| anyhow!("invalid modio URL {url}"))?;

        if let (Some(mod_id), Some(_modfile_id)) =
            (captures.name("mod_id"), captures.name("modfile_id"))
        {
            // both mod ID and modfile ID specified, but not necessarily name
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();

            let mod_ = if let Some(mod_) = (!update)
                .then(|| {
                    cache
                        .read()
                        .unwrap()
                        .get::<ModioCache>(MODIO_PROVIDER_ID)
                        .and_then(|c| c.mods.get(&mod_id).cloned())
                })
                .flatten()
            {
                mod_
            } else {
                let mod_: ModioMod = self
                    .modio
                    .game(MODIO_DRG_ID)
                    .mod_(mod_id)
                    .get()
                    .await?
                    .into();

                let mut lock = cache.write().unwrap();
                let c = lock.get_mut::<ModioCache>(MODIO_PROVIDER_ID);
                c.mods.insert(mod_id, mod_.clone());
                c.mod_id_map.insert(mod_.name_id.to_owned(), mod_id);

                mod_
            };

            let deps = match (!update)
                .then(|| {
                    cache
                        .read()
                        .unwrap()
                        .get::<ModioCache>(MODIO_PROVIDER_ID)
                        .and_then(|c| c.dependencies.get(&mod_id).cloned())
                })
                .flatten()
            {
                Some(deps) => deps,
                None => {
                    let deps = self
                        .modio
                        .game(MODIO_DRG_ID)
                        .mod_(mod_id)
                        .dependencies()
                        .list()
                        .await?
                        .into_iter()
                        .map(|d| d.mod_id)
                        .collect::<Vec<_>>();

                    cache
                        .write()
                        .unwrap()
                        .get_mut::<ModioCache>(MODIO_PROVIDER_ID)
                        .dependencies
                        .insert(mod_id, deps.clone());

                    deps
                }
            }
            .into_iter()
            .map(|d| format!("https://mod.io/g/drg/m/FIXME#{d}")) // since we found mod based on
            // ID, we haven't verified mod
            // name is actually correct
            .collect();

            Ok(ModResponse::Resolve(Mod {
                url: url.to_owned(),
                status: ResolvableStatus::Resolvable(ModResolution {
                    url: url.to_owned(),
                }),
                suggested_require: mod_.tags.contains("RequiredByAll"),
                suggested_dependencies: deps,
            }))
        } else if let Some(mod_id) = captures.name("mod_id") {
            // only mod ID specified, use latest version (either cached local or remote depending)
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();

            let cached = (!update)
                .then(|| {
                    cache
                        .read()
                        .unwrap()
                        .get::<ModioCache>(MODIO_PROVIDER_ID)
                        .and_then(|c| c.mods.get(&mod_id).cloned())
                })
                .flatten();

            let mod_ = if let Some(mod_) = cached {
                mod_
            } else {
                let mod_: ModioMod = self
                    .modio
                    .game(MODIO_DRG_ID)
                    .mod_(mod_id)
                    .get()
                    .await?
                    .into();

                let mut lock = cache.write().unwrap();
                let c = lock.get_mut::<ModioCache>(MODIO_PROVIDER_ID);
                c.mods.insert(mod_id, mod_.clone());
                c.mod_id_map.insert(mod_.name_id.to_owned(), mod_id);

                mod_
            };

            Ok(ModResponse::Redirect {
                url: format!(
                    "https://mod.io/g/drg/m/{}#{}/{}",
                    mod_.name_id,
                    mod_id,
                    mod_.latest_modfile.ok_or_else(|| anyhow!(
                        "mod {} does not have an associated modfile",
                        url
                    ))?
                ),
            })
        } else {
            use modio::filter::{Eq, In};
            use modio::mods::filters::{NameId, Visible};

            let name_id = captures.name("name_id").unwrap().as_str();

            let cached_id = if update {
                None
            } else {
                cache
                    .read()
                    .unwrap()
                    .get::<ModioCache>(MODIO_PROVIDER_ID)
                    .and_then(|c| c.mod_id_map.get(name_id).cloned())
            };

            if let Some(id) = cached_id {
                let cached = (!update)
                    .then(|| {
                        cache
                            .read()
                            .unwrap()
                            .get::<ModioCache>(MODIO_PROVIDER_ID)
                            .and_then(|c| c.mods.get(&id))
                            .and_then(|m| m.latest_modfile)
                    })
                    .flatten();

                let modfile_id = if let Some(modfile_id) = cached {
                    modfile_id
                } else {
                    let mod_: ModioMod = self.modio.game(MODIO_DRG_ID).mod_(id).get().await?.into();

                    let mut lock = cache.write().unwrap();
                    let c = lock.get_mut::<ModioCache>(MODIO_PROVIDER_ID);
                    c.mods.insert(id, mod_.clone());
                    c.mod_id_map.insert(mod_.name_id, id);

                    mod_.latest_modfile
                        .ok_or_else(|| anyhow!("mod {} does not have an associated modfile", url))?
                };

                Ok(ModResponse::Redirect {
                    url: format!("https://mod.io/g/drg/m/{}#{}/{}", &name_id, id, modfile_id),
                })
            } else {
                let filter = NameId::eq(name_id).and(Visible::_in(vec![0, 1]));
                let mut mods = self
                    .modio
                    .game(MODIO_DRG_ID)
                    .mods()
                    .search(filter)
                    .collect()
                    .await?;
                if mods.len() > 1 {
                    Err(anyhow!(
                        "multiple mods returned for mod name_id {}",
                        name_id,
                    ))
                } else if let Some(mod_) = mods.pop() {
                    let mod_id = mod_.id;
                    let mod_: ModioMod = self
                        .modio
                        .game(MODIO_DRG_ID)
                        .mod_(mod_id)
                        .get()
                        .await?
                        .into();

                    let mut lock = cache.write().unwrap();
                    let c = lock.get_mut::<ModioCache>(MODIO_PROVIDER_ID);
                    c.mods.insert(mod_id, mod_.clone());
                    c.mod_id_map.insert(mod_.name_id, mod_id);

                    let file = mod_.latest_modfile.ok_or_else(|| {
                        anyhow!("mod {} does not have an associated modfile", url)
                    })?;

                    Ok(ModResponse::Redirect {
                        url: format!("https://mod.io/g/drg/m/{}#{}/{}", &name_id, mod_id, file),
                    })
                } else {
                    Err(anyhow!("no mods returned for mod name_id {}", &name_id))
                }
            }
        }
    }
    async fn fetch_mod(
        &self,
        url: &str,
        _update: bool,
        cache: Arc<RwLock<ConfigWrapper<Cache>>>,
        blob_cache: &BlobCache,
    ) -> Result<PathBuf> {
        let captures = RE_MOD
            .captures(url)
            .ok_or_else(|| anyhow!("invalid modio URL {url}"))?;

        if let (Some(_name_id), Some(mod_id), Some(modfile_id)) = (
            captures.name("name_id"),
            captures.name("mod_id"),
            captures.name("modfile_id"),
        ) {
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();
            let modfile_id = modfile_id.as_str().parse::<u32>().unwrap();

            Ok(
                if let Some(path) = {
                    let path = cache
                        .read()
                        .unwrap()
                        .get::<ModioCache>(MODIO_PROVIDER_ID)
                        .and_then(|c| c.modfile_blobs.get(&modfile_id))
                        .and_then(|r| blob_cache.get_path(r));
                    path
                } {
                    path
                } else {
                    let file = self
                        .modio
                        .game(MODIO_DRG_ID)
                        .mod_(mod_id)
                        .file(modfile_id)
                        .get()
                        .await?;

                    let download: modio::download::DownloadAction = file.into();

                    println!("downloading mod {url}...");

                    let data = self.modio.download(download).bytes().await?.to_vec();
                    let blob = blob_cache.write(&data)?;
                    let path = blob_cache.get_path(&blob).unwrap();

                    cache
                        .write()
                        .unwrap()
                        .get_mut::<ModioCache>(MODIO_PROVIDER_ID)
                        .modfile_blobs
                        .insert(modfile_id, blob);

                    path
                },
            )
        } else {
            Err(anyhow!("download URL must be fully specified"))
        }
    }
}
