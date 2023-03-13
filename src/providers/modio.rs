use std::sync::Arc;
use std::{collections::HashMap, io::Cursor};

use anyhow::{anyhow, Result};
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};
use serde::{Deserialize, Serialize};
use task_local_extensions::Extensions;
use tokio::sync::RwLock;

use super::{
    BlobCache, BlobRef, CacheWrapper, ModProvider, ModProviderCache, ModResponse, ResolvableStatus,
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
    latest_modfile: HashMap<u32, u32>,
    modfile_blobs: HashMap<u32, BlobRef>,
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
        cache: Arc<RwLock<CacheWrapper>>,
        blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let mut inner = cache.write().await;
        let cache = inner.get_mut::<ModioCache>("modio");

        let captures = RE_MOD
            .captures(url)
            .ok_or_else(|| anyhow!("invalid modio URL {url}"))?;

        if let (Some(mod_id), Some(modfile_id)) =
            (captures.name("mod_id"), captures.name("modfile_id"))
        {
            let mod_id = mod_id.as_str().parse::<u32>().unwrap();
            let modfile_id = modfile_id.as_str().parse::<u32>().unwrap();

            let data = if let Some(blob) = cache
                .modfile_blobs
                .get(&modfile_id)
                .and_then(|r| blob_cache.read(r).ok())
            {
                blob
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
                cache
                    .modfile_blobs
                    .insert(modfile_id, blob_cache.write(&data)?);

                Box::new(Cursor::new(data))
            };

            Ok(ModResponse::Resolve {
                status: ResolvableStatus::Resolvable {
                    url: url.to_owned(),
                },
                data,
            })
        } else if let Some(mod_id) = captures.name("mod_id") {
            let name_id = captures.name("name_id").unwrap().as_str();

            let mod_ = self
                .modio
                .game(MODIO_DRG_ID)
                .mod_(mod_id.as_str().parse::<u32>().unwrap())
                .get()
                .await?;

            let file = mod_
                .modfile
                .ok_or_else(|| anyhow!("mod {} does not have an associated modfile", url))?;

            Ok(ModResponse::Redirect {
                url: format!(
                    "https://mod.io/g/drg/m/{}#{}/{}",
                    &name_id, file.mod_id, file.id
                ),
            })
        } else {
            use modio::filter::{Eq, In};
            use modio::mods::filters::{NameId, Visible};

            let name_id = captures.name("name_id").unwrap().as_str();

            let cached_id = cache.mod_id_map.get(name_id);

            if let Some(id) = cached_id {
                if let Some(modfile_id) = cache.latest_modfile.get(id) {
                    Ok(ModResponse::Redirect {
                        url: format!("https://mod.io/g/drg/m/{}#{}/{}", &name_id, id, modfile_id),
                    })
                } else {
                    let mod_ = self.modio.mod_(MODIO_DRG_ID, *id).get().await?;
                    let file = mod_.modfile.ok_or_else(|| {
                        anyhow!("mod {} does not have an associated modfile", url)
                    })?;
                    cache.latest_modfile.insert(*id, file.id);
                    Ok(ModResponse::Redirect {
                        url: format!("https://mod.io/g/drg/m/{}#{}/{}", &name_id, id, file.id),
                    })
                }
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
                    cache.mod_id_map.insert(name_id.to_owned(), mod_.id);
                    let file = mod_.modfile.ok_or_else(|| {
                        anyhow!("mod {} does not have an associated modfile", url)
                    })?;
                    cache.latest_modfile.insert(mod_.id, file.id);

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
