use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};
use serde::{Deserialize, Serialize};
use task_local_extensions::Extensions;

use super::{
    BlobCache, BlobRef, CacheWrapper, Mod, ModProvider, ModProviderCache, ModResponse,
    ResolvableStatus,
};

inventory::submit! {
    super::ProviderFactory {
        id: "modio",
        new: ModioProvider::new_provider,
        can_provide: |url| RE_MOD.is_match(&url),
    }
}

#[derive(Debug)]
pub struct ModioProvider {
    modio: modio::Modio,
}

impl ModioProvider {
    fn new_provider() -> Result<Box<dyn ModProvider>> {
        if let Ok(modio_key) = std::env::var("MODIO_KEY") {
            let token = std::env::var("MODIO_ACCESS_TOKEN").ok();
            let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
                .with(LoggingMiddleware {
                    requests: Default::default(),
                })
                .build();
            let modio = modio::Modio::new(
                if let Some(token) = token {
                    modio::Credentials::with_token(modio_key, token)
                } else {
                    modio::Credentials::new(modio_key)
                },
                client,
            )?;

            Ok(Box::new(Self::new(modio)))
        } else {
            Err(anyhow!(
                "MODIO_KEY env var not found, modio provider will be unavailable"
            ))
        }
    }
    fn new(modio: modio::Modio) -> Self {
        Self { modio }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ModioCache {
    mod_id_map: HashMap<String, u32>,
    name_id_map: HashMap<u32, String>,
    latest_modfile: HashMap<u32, u32>,
    modfile_blobs: HashMap<u32, BlobRef>,
    dependencies: HashMap<u32, Vec<u32>>,
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

lazy_static::lazy_static! {
    static ref RE_MOD: regex::Regex = regex::Regex::new("^https://mod.io/g/drg/m/(?P<name_id>[^/#]+)(:?#(?P<mod_id>\\d+)(:?/(?P<modfile_id>\\d+))?)?$").unwrap();
}

const MODIO_DRG_ID: u32 = 2475;

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
    async fn get_mod(
        &self,
        url: &str,
        update: bool,
        cache: Arc<RwLock<CacheWrapper>>,
        blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let pid = "modio";

        let captures = RE_MOD
            .captures(url)
            .ok_or_else(|| anyhow!("invalid modio URL {url}"))?;

        if let (Some(mod_id), Some(modfile_id)) =
            (captures.name("mod_id"), captures.name("modfile_id"))
        {
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();
            let modfile_id = modfile_id.as_str().parse::<u32>().unwrap();

            let path = if let Some(path) = {
                let lock = cache.read().unwrap();
                let path = lock
                    .get::<ModioCache>(pid)
                    .and_then(|c| c.modfile_blobs.get(&modfile_id))
                    .and_then(|r| blob_cache.get_path(r));
                drop(lock);
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
                    .get_mut::<ModioCache>(pid)
                    .modfile_blobs
                    .insert(modfile_id, blob);

                path
            };

            let deps = match (!update)
                .then(|| {
                    cache
                        .read()
                        .unwrap()
                        .get::<ModioCache>(pid)
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
                        .get_mut::<ModioCache>(pid)
                        .dependencies
                        .insert(mod_id, deps.clone());

                    deps
                }
            }
            .into_iter()
            .map(|d| format!("https://mod.io/g/drg/m/FIXME#{d}"))
            .collect();

            Ok(ModResponse::Resolve(Mod {
                status: ResolvableStatus::Resolvable {
                    url: url.to_owned(),
                },
                path,
                suggested_require: false,
                suggested_dependencies: deps,
            }))
        } else if let Some(mod_id) = captures.name("mod_id") {
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();

            let cached = (!update)
                .then(|| {
                    cache.read().unwrap().get::<ModioCache>(pid).and_then(|c| {
                        c.latest_modfile
                            .get(&mod_id)
                            .copied()
                            .zip(c.name_id_map.get(&mod_id).map(|n| n.to_owned()))
                    })
                })
                .flatten();

            let mod_ = if let Some((file_id, name_id)) = cached {
                (name_id, Some(file_id))
            } else {
                let mod_ = self.modio.game(MODIO_DRG_ID).mod_(mod_id).get().await?;

                let mut lock = cache.write().unwrap();
                let c = lock.get_mut::<ModioCache>(pid);
                c.name_id_map.insert(mod_id, mod_.name_id.to_owned());
                if let Some(modfile) = &mod_.modfile {
                    c.latest_modfile.insert(mod_id, modfile.id);
                }

                (mod_.name_id, mod_.modfile.map(|f| f.id))
            };

            Ok(ModResponse::Redirect {
                url: format!(
                    "https://mod.io/g/drg/m/{}#{}/{}",
                    mod_.0,
                    mod_id,
                    mod_.1.ok_or_else(|| anyhow!(
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
                    .get::<ModioCache>(pid)
                    .and_then(|c| c.mod_id_map.get(name_id).cloned())
            };

            if let Some(id) = cached_id {
                let modfile_id = if let Some(modfile_id) = if update {
                    None
                } else {
                    cache
                        .read()
                        .unwrap()
                        .get::<ModioCache>(pid)
                        .and_then(|c| c.latest_modfile.get(&id).cloned())
                } {
                    modfile_id
                } else {
                    let modfile_id = self
                        .modio
                        .mod_(MODIO_DRG_ID, id)
                        .get()
                        .await?
                        .modfile
                        .ok_or_else(|| anyhow!("mod {} does not have an associated modfile", url))?
                        .id;

                    cache
                        .write()
                        .unwrap()
                        .get_mut::<ModioCache>(pid)
                        .latest_modfile
                        .insert(id, modfile_id);

                    modfile_id
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
                    cache
                        .write()
                        .unwrap()
                        .get_mut::<ModioCache>(pid)
                        .mod_id_map
                        .insert(name_id.to_owned(), mod_.id);
                    let file = mod_.modfile.ok_or_else(|| {
                        anyhow!("mod {} does not have an associated modfile", url)
                    })?;
                    cache
                        .write()
                        .unwrap()
                        .get_mut::<ModioCache>(pid)
                        .latest_modfile
                        .insert(mod_.id, file.id);

                    Ok(ModResponse::Redirect {
                        url: format!(
                            "https://mod.io/g/drg/m/{}#{}/{}",
                            &name_id, file.mod_id, file.id
                        ),
                    })
                } else {
                    Err(anyhow!("no mods returned for mod name_id {}", &name_id))
                }
            }
        }
    }
}
