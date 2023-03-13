use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{
    BlobCache, BlobRef, CacheWrapper, ModProvider, ModProviderCache, ModResponse, ResolvableStatus,
};

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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HttpProviderCache {
    url_blobs: HashMap<String, BlobRef>,
}
#[typetag::serde]
impl ModProviderCache for HttpProviderCache {
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
    async fn get_mod(
        &self,
        url: &str,
        cache: Arc<RwLock<CacheWrapper>>,
        blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let mut wrapper = cache.write().await;
        let cache = wrapper.get_mut::<HttpProviderCache>("http");
        if let Some(blob) = cache
            .url_blobs
            .get(url)
            .and_then(|r| blob_cache.read(r).ok())
        {
            Ok(ModResponse::Resolve {
                status: ResolvableStatus::Resolvable {
                    url: url.to_owned(),
                },
                data: blob,
            })
        } else {
            println!("downloading mod {url}...");
            Ok(ModResponse::Resolve {
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

                    let data = res.bytes().await?.to_vec();
                    cache
                        .url_blobs
                        .insert(url.to_owned(), blob_cache.write(&data)?);

                    data
                })),
            })
        }
    }
}
