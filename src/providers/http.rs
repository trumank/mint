use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::{
    BlobCache, BlobRef, ModInfo, ModProvider, ModProviderCache, ModResolution, ModResponse,
    ModSpecification, ProviderCache, ResolvableStatus,
};

inventory::submit! {
    super::ProviderFactory {
        id: "http",
        new: HttpProvider::new_provider,
        can_provide: |url| -> bool {
            RE_MOD
                .captures(url)
                .and_then(|c| c.name("hostname"))
                .map_or(false, |h| {
                    !["mod.io", "drg.mod.io", "drg.old.mod.io"].contains(&h.as_str())
                })
        },
        parameters: &[],
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
    pub fn new_provider(_parameters: &HashMap<String, String>) -> Result<Arc<dyn ModProvider>> {
        Ok(Arc::new(Self::new()))
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

const HTTP_PROVIDER_ID: &str = "http";

#[async_trait::async_trait]
impl ModProvider for HttpProvider {
    async fn resolve_mod(
        &self,
        spec: &ModSpecification,
        _update: bool,
        _cache: ProviderCache,
        _blob_cache: &BlobCache,
    ) -> Result<ModResponse> {
        let url = url::Url::parse(&spec.url)?;
        let name = url
            .path_segments()
            .and_then(|s| s.last())
            .map(|s| s.to_string())
            .unwrap_or_else(|| url.to_string());
        Ok(ModResponse::Resolve(ModInfo {
            provider: HTTP_PROVIDER_ID,
            name,
            spec: spec.clone(),
            versions: vec![spec.clone()],
            status: ResolvableStatus::Resolvable(ModResolution {
                url: spec.url.to_owned(),
            }),
            suggested_require: false,
            suggested_dependencies: vec![],
        }))
    }

    async fn fetch_mod(
        &self,
        res: &ModResolution,
        update: bool,
        cache: ProviderCache,
        blob_cache: &BlobCache,
    ) -> Result<PathBuf> {
        let url = &res.url;
        Ok(
            if let Some(path) = if update {
                None
            } else {
                cache
                    .read()
                    .unwrap()
                    .get::<HttpProviderCache>(HTTP_PROVIDER_ID)
                    .and_then(|c| c.url_blobs.get(url))
                    .and_then(|r| blob_cache.get_path(r))
            } {
                path
            } else {
                println!("downloading mod {url}...");
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
                    .get_mut::<HttpProviderCache>(HTTP_PROVIDER_ID)
                    .url_blobs
                    .insert(url.to_owned(), blob);

                path
            },
        )
    }
    fn get_mod_info(&self, spec: &ModSpecification, _cache: ProviderCache) -> Option<ModInfo> {
        let url = url::Url::parse(&spec.url).ok()?;
        let name = url
            .path_segments()
            .and_then(|s| s.last())
            .map(|s| s.to_string())
            .unwrap_or_else(|| url.to_string());
        Some(ModInfo {
            provider: HTTP_PROVIDER_ID,
            name,
            spec: spec.clone(),
            versions: vec![spec.clone()],
            status: ResolvableStatus::Resolvable(ModResolution {
                url: spec.url.to_owned(),
            }),
            suggested_require: false,
            suggested_dependencies: vec![],
        })
    }

    fn is_pinned(&self, _spec: &ModSpecification, _cache: ProviderCache) -> bool {
        true
    }
}
