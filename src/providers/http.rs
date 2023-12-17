use std::path::PathBuf;
use std::sync::OnceLock;
use std::{collections::HashMap, sync::Arc};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;
use tracing::info;

use super::{
    BlobCache, BlobRef, FetchProgress, ModInfo, ModProvider, ModProviderCache, ModResolution,
    ModResponse, ModSpecification, ProviderCache,
};

inventory::submit! {
    super::ProviderFactory {
        id: "http",
        new: HttpProvider::new_provider,
        can_provide: |url| -> bool {
            re_mod()
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

static RE_MOD: OnceLock<regex::Regex> = OnceLock::new();
fn re_mod() -> &'static regex::Regex {
    RE_MOD.get_or_init(|| regex::Regex::new(r"^https?://(?P<hostname>[^/]+)(/|$)").unwrap())
}

const HTTP_PROVIDER_ID: &str = "http";

#[async_trait::async_trait]
impl ModProvider for HttpProvider {
    async fn resolve_mod(
        &self,
        spec: &ModSpecification,
        _update: bool,
        _cache: ProviderCache,
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
            versions: vec![],
            resolution: ModResolution::resolvable(spec.url.to_owned()),
            suggested_require: false,
            suggested_dependencies: vec![],
            modio_tags: None,
            modio_id: None,
        }))
    }

    async fn fetch_mod(
        &self,
        res: &ModResolution,
        update: bool,
        cache: ProviderCache,
        blob_cache: &BlobCache,
        tx: Option<Sender<FetchProgress>>,
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
                if let Some(tx) = tx {
                    tx.send(FetchProgress::Complete {
                        resolution: res.clone(),
                    })
                    .await
                    .unwrap();
                }
                path
            } else {
                info!("downloading mod {url}...");
                let response = self.client.get(url).send().await?.error_for_status()?;
                let size = response.content_length(); // TODO will be incorrect if compressed
                if let Some(mime) = response
                    .headers()
                    .get(reqwest::header::HeaderName::from_static("content-type"))
                {
                    let content_type = &mime.to_str()?;
                    if !["application/zip", "application/octet-stream"].contains(content_type) {
                        bail!("unexpected content-type: {content_type}");
                    }
                }

                use futures::stream::TryStreamExt;
                use tokio::io::AsyncWriteExt;

                let mut cursor = std::io::Cursor::new(vec![]);
                let mut stream = response.bytes_stream();
                while let Some(bytes) = stream.try_next().await? {
                    cursor.write_all(&bytes).await?;
                    if let Some(size) = size {
                        if let Some(tx) = &tx {
                            tx.send(FetchProgress::Progress {
                                resolution: res.clone(),
                                progress: cursor.get_ref().len() as u64,
                                size,
                            })
                            .await
                            .unwrap();
                        }
                    }
                }

                let blob = blob_cache.write(&cursor.into_inner())?;
                let path = blob_cache.get_path(&blob).unwrap();
                cache
                    .write()
                    .unwrap()
                    .get_mut::<HttpProviderCache>(HTTP_PROVIDER_ID)
                    .url_blobs
                    .insert(url.to_owned(), blob);

                if let Some(tx) = tx {
                    tx.send(FetchProgress::Complete {
                        resolution: res.clone(),
                    })
                    .await
                    .unwrap();
                }
                path
            },
        )
    }

    async fn update_cache(&self, _cache: ProviderCache) -> Result<()> {
        Ok(())
    }

    async fn check(&self) -> Result<()> {
        Ok(())
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
            versions: vec![],
            resolution: ModResolution::resolvable(spec.url.to_owned()),
            suggested_require: false,
            suggested_dependencies: vec![],
            modio_tags: None,
            modio_id: None,
        })
    }

    fn is_pinned(&self, _spec: &ModSpecification, _cache: ProviderCache) -> bool {
        true
    }
    fn get_version_name(&self, _spec: &ModSpecification, _cache: ProviderCache) -> Option<String> {
        Some("latest".to_string())
    }
}
