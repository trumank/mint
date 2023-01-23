use crate::ModId;

use serde::{Deserialize, Serialize};

use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ModioCache {
    pub mods: HashMap<ModId, ModioMod>,
    pub dependencies: HashMap<ModId, Vec<ModId>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModioMod {
    pub id: ModId,
    pub name: String,
    pub tags: Vec<String>,
    pub versions: BTreeMap<u32, ModioFile>,
    pub url: url::Url,
}

impl From<modio::mods::Mod> for ModioMod {
    fn from(value: modio::mods::Mod) -> Self {
        ModioMod {
            id: ModId(value.id),
            name: value.name,
            tags: value.tags.into_iter().map(|t| t.name).collect(),
            versions: value.modfile.map(|f| (f.id, f.into())).into_iter().collect(),
            url: value.profile_url,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModioFile {
    pub filehash: String,
}

impl From<modio::files::File> for ModioFile {
    fn from(value: modio::files::File) -> Self {
        ModioFile {
            filehash: value.filehash.md5,
        }
    }
}
