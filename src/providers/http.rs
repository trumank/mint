use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::{
    BlobCache, BlobRef, CacheWrapper, Mod, ModProvider, ModProviderCache, ModResponse,
    ResolvableStatus,
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
        update: bool,
        cache: Arc<RwLock<CacheWrapper>>,
        blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let pid = "http";

        if let Some(path) = if update {
            None
        } else {
            cache
                .read()
                .unwrap()
                .get::<HttpProviderCache>(pid)
                .and_then(|c| c.url_blobs.get(url))
                .and_then(|r| blob_cache.get_path(r))
        } {
            Ok(ModResponse::Resolve(Mod {
                status: ResolvableStatus::Resolvable {
                    url: url.to_owned(),
                },
                path,
                suggested_require: false,
                suggested_dependencies: vec![],
            }))
        } else {
            println!("downloading mod {url}...");
            Ok(ModResponse::Resolve(Mod {
                status: ResolvableStatus::Resolvable {
                    url: url.to_owned(),
                },
                path: {
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
                    let blob = blob_cache.write(&data)?;
                    let path = blob_cache.get_path(&blob).unwrap();
                    cache
                        .write()
                        .unwrap()
                        .get_mut::<HttpProviderCache>(pid)
                        .url_blobs
                        .insert(url.to_owned(), blob);

                    path
                },
                suggested_require: false,
                suggested_dependencies: vec![],
            }))
        }
    }
}
