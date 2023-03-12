pub mod file;
pub mod http;
pub mod modio;

use crate::error::IntegrationError;

use anyhow::{anyhow, Result};

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek};
use std::path::{Path, PathBuf};

fn hash_string(input: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct ModStore {
    cache_path: PathBuf,
    providers: HashMap<ProviderFactory, Box<dyn ModProvider>>,
}
impl ModStore {
    pub fn new<P: AsRef<Path>>(cache_path: P) -> Self {
        ModStore {
            cache_path: cache_path.as_ref().to_path_buf(),
            providers: HashMap::new(),
        }
    }
    pub fn add_provider(&mut self, provider_factory: ProviderFactory) -> Result<()> {
        let provider = (provider_factory.new)()?;
        self.providers.insert(provider_factory, provider);
        Ok(())
    }
    pub fn get_provider(&self, url: &str) -> Result<&Box<dyn ModProvider>> {
        let factory = inventory::iter::<ProviderFactory>()
            .find(|f| (f.can_provide)(url.to_owned()))
            .ok_or_else(|| anyhow!("Could not find mod provider for {}", url))?;
        let entry = self.providers.get(factory);
        Ok(match entry {
            Some(e) => e,
            None => {
                return Err(IntegrationError::NoProvider {
                    url: url.to_owned(),
                    factory: factory.clone(),
                }
                .into())
            }
        })
    }
    pub async fn get_mod(&self, mut url: String) -> Result<Mod> {
        loop {
            let path = self.cache_path.join(hash_string(&url));

            match File::open(&path) {
                Ok(data) => {
                    return Ok(Mod {
                        status: ResolvableStatus::Resolvable {
                            url: url.to_owned(),
                        },
                        data: Box::new(BufReader::new(data)),
                    })
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                    match self.get_provider(&url)?.get_mod(&url).await? {
                        ModResponse::Resolve {
                            cache,
                            mut data,
                            status,
                        } => {
                            let data: Box<dyn ReadSeek> = if cache {
                                println!("caching url: {url}");
                                let mut cache_file = OpenOptions::new()
                                    .read(true)
                                    .write(true)
                                    .create(true)
                                    .truncate(true)
                                    .open(self.cache_path.join(hash_string(&url)))?;
                                std::io::copy(&mut data, &mut BufWriter::new(&cache_file))?;
                                cache_file.rewind()?;
                                Box::new(BufReader::new(cache_file))
                            } else {
                                data
                            };
                            return Ok(Mod { status, data });
                        }
                        ModResponse::Redirect {
                            url: redirected_url,
                        } => url = redirected_url,
                    };
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

pub trait ReadSeek: Read + Seek {}
impl<T: Seek + Read> ReadSeek for T {}

/// Whether a mod can be resolved by clients or not
#[derive(Debug)]
pub enum ResolvableStatus {
    /// If a mod can not be resolved, specify just a name
    Unresolvable { name: String },
    /// Ifa mod can be resolved, specify the URL
    Resolvable { url: String },
}

/// Returned from ModStore
pub struct Mod {
    pub status: ResolvableStatus,
    pub data: Box<dyn ReadSeek>,
}

/// Returned from ModProvider
pub enum ModResponse {
    Redirect {
        url: String,
    },
    Resolve {
        cache: bool,
        status: ResolvableStatus,
        data: Box<dyn ReadSeek>,
    },
}

#[async_trait::async_trait]
pub trait ModProvider: Sync + std::fmt::Debug {
    async fn get_mod(&self, url: &str) -> Result<ModResponse>;
}

/*
pub trait ModProviderFactory: Sync + std::fmt::Debug {
    fn build() -> Result<Box<dyn ModProvider>>;
    fn can_provide(url: &str) -> bool;
}
*/
#[derive(Debug, Clone, Eq, Ord, Hash, PartialEq, PartialOrd)]
pub struct ProviderFactory {
    new: fn() -> Result<Box<dyn ModProvider>>,
    can_provide: fn(String) -> bool,
}

inventory::collect!(ProviderFactory);
