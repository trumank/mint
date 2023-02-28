use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{anyhow, Result};

use super::{ModProvider, ModResponse, ResolvableStatus};

pub struct FileProvider {}

#[async_trait::async_trait]
impl ModProvider for FileProvider {
    fn can_provide(&self, url: &str) -> bool {
        Path::new(url).exists()
    }

    async fn get_mod(&self, url: &str) -> Result<ModResponse> {
        let path = Path::new(url);
        Ok(ModResponse::Resolve {
            cache: false,
            status: ResolvableStatus::Unresolvable {
                name: path
                    .file_name()
                    .ok_or_else(|| anyhow!("could not determine file name of {}", url))?
                    .to_string_lossy()
                    .to_string(),
            },
            data: Box::new(BufReader::new(File::open(path)?)),
        })
    }
}
