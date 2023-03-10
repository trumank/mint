use std::io::Cursor;

use anyhow::{anyhow, Result};
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};
use task_local_extensions::Extensions;

use super::{ModProvider, ModResponse, ResolvableStatus};

inventory::submit! {
    super::ProviderFactory(ModioProvider::new_provider)
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
    fn can_provide(&self, url: &str) -> bool {
        RE_MOD.is_match(url)
    }

    async fn get_mod(&self, url: &str) -> Result<ModResponse> {
        let captures = RE_MOD
            .captures(url)
            .ok_or_else(|| anyhow!("invalid modio URL {url}"))?;

        if let (Some(mod_id), Some(modfile_id)) =
            (captures.name("mod_id"), captures.name("modfile_id"))
        {
            let file = self
                .modio
                .game(MODIO_DRG_ID)
                .mod_(mod_id.as_str().parse::<u32>().unwrap())
                .file(modfile_id.as_str().parse::<u32>().unwrap())
                .get()
                .await?;

            let download: modio::download::DownloadAction = file.into();

            println!("downloading mod {url}...");

            let data = Box::new(Cursor::new(
                self.modio.download(download).bytes().await?.to_vec(),
            ));

            Ok(ModResponse::Resolve {
                cache: true,
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
                Err(anyhow!("no mods returned for mod name_id {}", &name_id))
            }
        }
    }
}
