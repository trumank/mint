use std::io::Cursor;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::RwLock;

use super::{CacheWrapper, ModProvider, ModResponse, ResolvableStatus};

inventory::submit! {
    super::ProviderFactory {
        id: "http",
        new: HttpProvider::new_provider,
        can_provide: |url| -> bool {
            RE_MOD
                .captures(&url)
                .and_then(|c| c.name("hostname"))
                .map_or(false, |h| {
                    !["mod.io", "drg.mod.io", "drg.old.mod.io"].contains(&h.as_str())
                })
        }
    }
}

#[derive(Debug)]
pub struct HttpProvider {
    client: reqwest::Client,
}

impl HttpProvider {
    pub fn new_provider() -> Result<Box<dyn ModProvider>> {
        Ok(Box::new(Self::new()))
    }
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

lazy_static::lazy_static! {
    static ref RE_MOD: regex::Regex = regex::Regex::new(r"^https?://(?P<hostname>[^/]+)(/|$)").unwrap();
}

#[async_trait::async_trait]
impl ModProvider for HttpProvider {
    async fn get_mod(&self, url: &str, _cache: Arc<RwLock<CacheWrapper>>) -> Result<ModResponse> {
        println!("downloading mod {url}...");
        Ok(ModResponse::Resolve {
            cache: true,
            status: ResolvableStatus::Resolvable {
                url: url.to_owned(),
            },
            data: Box::new(Cursor::new({
                let res = self.client.get(url).send().await?.error_for_status()?;
                if let Some(mime) = res
                    .headers()
                    .get(reqwest::header::HeaderName::from_static("content-type"))
                {
                    let content_type = &mime.to_str()?;
                    if !["application/zip", "application/octet-stream"].contains(content_type) {
                        return Err(anyhow!("unexpected content-type: {content_type}"));
                    }
                }
                res.bytes().await?.to_vec()
            })),
        })
    }
}
