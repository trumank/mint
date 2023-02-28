use std::io::Cursor;

use anyhow::{anyhow, Result};

use super::{ModProvider, ModResponse, ResolvableStatus};

pub struct HttpProvider {
    client: reqwest::Client,
}

impl HttpProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl ModProvider for HttpProvider {
    fn can_provide(&self, url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    async fn get_mod(&self, url: &str) -> Result<ModResponse> {
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
