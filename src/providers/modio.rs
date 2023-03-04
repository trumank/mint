use std::io::Cursor;

use anyhow::{anyhow, Result};

use super::{ModProvider, ModResponse, ResolvableStatus};

pub struct ModioProvider {
    modio: modio::Modio,
}

impl ModioProvider {
    pub fn new(modio: modio::Modio) -> Self {
        Self { modio }
    }
}

lazy_static::lazy_static! {
    static ref RE_MOD: regex::Regex = regex::Regex::new("^https://mod.io/g/drg/m/(?P<name_id>[^/#]+)(:?#(?P<mod_id>\\d+)(:?/(?P<modfile_id>\\d+))?)?$").unwrap();
}

const MODIO_DRG_ID: u32 = 2475;

#[async_trait::async_trait]
impl ModProvider for ModioProvider {
    fn can_provide(&self, url: &str) -> bool {
        //RE_MOD.is_match(url)
        url.starts_with("https://mod.io/") // allow possibly invalid URLs so it doesn't get passed
                                           // to the generic HTTP provider
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
