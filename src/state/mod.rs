pub mod config;

use std::ops::DerefMut;
use std::{
    collections::{BTreeMap, HashMap},
    ops::Deref,
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use indexmap::IndexMap;

use crate::{
    providers::{ModSpecification, ModStore},
    DRGInstallation,
};

use self::config::ConfigWrapper;

#[obake::versioned]
#[obake(version("0.0.0"))]
#[obake(version("0.1.0"))]
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModProfile {
    #[obake(cfg("0.0.0"))]
    pub mods: Vec<ModConfig>,

    /// Mapping between mod groups and if they are enabled for a profile.
    #[obake(cfg("0.1.0"))]
    pub mod_groups: IndexMap<String, bool>,
}

#[obake::versioned]
#[obake(version("0.0.0"))]
#[obake(version("0.1.0"))]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ModData {
    pub active_profile: String,
    #[obake(cfg("0.0.0"))]
    pub profiles: BTreeMap<String, ModProfile!["0.0.0"]>,
    #[obake(cfg("0.1.0"))]
    pub profiles: BTreeMap<String, ModProfile!["0.1.0"]>,
    #[obake(cfg("0.1.0"))]
    pub active_group: String,
    #[obake(cfg("0.1.0"))]
    pub groups: BTreeMap<String, ModGroup>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModGroup {
    pub mods: Vec<ModConfig>,
}

impl Default for ModData!["0.0.0"] {
    fn default() -> Self {
        Self {
            active_profile: "default".to_string(),
            profiles: [("default".to_string(), Default::default())]
                .into_iter()
                .collect(),
        }
    }
}

impl Default for ModData!["0.1.0"] {
    fn default() -> Self {
        Self {
            active_profile: "default".to_string(),
            profiles: [("default".to_string(), Default::default())]
                .into_iter()
                .collect(),
            active_group: "default".to_string(),
            groups: [("default".to_string(), Default::default())]
                .into_iter()
                .collect(),
        }
    }
}

impl From<ModData!["0.0.0"]> for ModData!["0.1.0"] {
    fn from(legacy: ModData!["0.0.0"]) -> Self {
        let mut new_mod_groups = Vec::new();
        for (name, profile) in &legacy.profiles {
            new_mod_groups.push((
                name.clone(),
                ModGroup {
                    mods: profile.mods.clone(),
                },
            ));
        }
        let mut new_profiles = Vec::new();
        for (name, _) in &legacy.profiles {
            let mut mod_groups = Vec::new();
            for (inner_name, _) in &legacy.profiles {
                if name == inner_name {
                    mod_groups.push((inner_name.clone(), true));
                } else {
                    mod_groups.push((inner_name.clone(), false));
                }
            }

            new_profiles.push((
                name.clone(),
                ModProfile_v0_1_0 {
                    mod_groups: mod_groups.into_iter().collect(),
                },
            ));
        }

        Self {
            active_profile: legacy.active_profile.clone(),
            profiles: new_profiles.into_iter().collect(),
            active_group: legacy.active_profile,
            groups: new_mod_groups.into_iter().collect(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version")]
pub enum VersionCheckedModData {
    #[serde(rename = "0")]
    Legacy(ModData!["0.0.0"]),
    #[serde(rename = "1")]
    V1(ModData!["0.1.0"]),
    #[serde(other)]
    Unsuppported,
}

impl From<ModProfile!["0.0.0"]> for ModProfile!["0.1.0"] {
    fn from(_legacy: ModProfile!["0.0.0"]) -> Self {
        // The migration requires `ModData` to handle instead.
        unimplemented!("migration requires handling from `ModData`")
    }
}

/// Mod configuration, holds ModSpecification as well as other metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModConfig {
    pub spec: ModSpecification,
    pub required: bool,

    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl ModData!["0.0.0"] {
    pub fn get_active_profile(&self) -> &ModProfile!["0.0.0"] {
        &self.profiles[&self.active_profile]
    }

    pub fn get_active_profile_mut(&mut self) -> &mut ModProfile!["0.0.0"] {
        self.profiles.get_mut(&self.active_profile).unwrap()
    }

    pub fn remove_active_profile(&mut self) {
        self.profiles.remove(&self.active_profile);
        self.active_profile = self.profiles.keys().next().unwrap().to_string();
    }
}

impl ModData!["0.1.0"] {
    pub fn get_active_profile(&self) -> &ModProfile!["0.1.0"] {
        &self.profiles[&self.active_profile]
    }

    pub fn get_active_profile_mut(&mut self) -> &mut ModProfile!["0.1.0"] {
        self.profiles.get_mut(&self.active_profile).unwrap()
    }

    pub fn remove_active_profile(&mut self) {
        self.profiles.remove(&self.active_profile);
        self.active_profile = self.profiles.keys().next().unwrap().to_string();
    }

    pub fn get_active_group(&self) -> &ModGroup {
        &self.groups[&self.active_group]
    }

    pub fn get_active_group_mut(&mut self) -> &mut ModGroup {
        self.groups.get_mut(&self.active_group).unwrap()
    }

    pub fn remove_active_group(&mut self) {
        self.groups.remove(&self.active_group);
        self.active_group = self.groups.keys().next().unwrap().to_string();
    }
}

#[obake::versioned]
#[obake(version("0.0.0"))]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub provider_parameters: HashMap<String, HashMap<String, String>>,
    pub drg_pak_path: Option<PathBuf>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version")]
pub enum VersionAnnotatedConfig {
    #[serde(rename = "0.0.0")]
    V0_0_0(Config!["0.0.0"]),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum MaybeVersionedConfig {
    Legacy(Config!["0.0.0"]),
    Versioned(VersionAnnotatedConfig),
}

impl Default for MaybeVersionedConfig {
    fn default() -> Self {
        MaybeVersionedConfig::Versioned(Default::default())
    }
}

impl Default for VersionAnnotatedConfig {
    fn default() -> Self {
        VersionAnnotatedConfig::V0_0_0(Default::default())
    }
}

impl Deref for VersionAnnotatedConfig {
    type Target = Config!["0.0.0"];

    fn deref(&self) -> &Self::Target {
        match self {
            VersionAnnotatedConfig::V0_0_0(cfg) => cfg,
        }
    }
}

impl DerefMut for VersionAnnotatedConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            VersionAnnotatedConfig::V0_0_0(cfg) => cfg,
        }
    }
}

impl Default for Config!["0.0.0"] {
    fn default() -> Self {
        Self {
            provider_parameters: Default::default(),
            drg_pak_path: DRGInstallation::find()
                .as_ref()
                .map(DRGInstallation::main_pak),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version")]
pub enum VersionAnnotatedModData {
    #[serde(rename = "0.1.0")]
    V0_1_0(ModData!["0.1.0"]),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum MaybeVersionedModData {
    Legacy(ModData!["0.0.0"]),
    Versioned(VersionAnnotatedModData),
}

impl Default for MaybeVersionedModData {
    fn default() -> Self {
        MaybeVersionedModData::Versioned(Default::default())
    }
}

impl Default for VersionAnnotatedModData {
    fn default() -> Self {
        VersionAnnotatedModData::V0_1_0(Default::default())
    }
}

impl Deref for VersionAnnotatedModData {
    type Target = ModData!["0.1.0"];

    fn deref(&self) -> &Self::Target {
        match self {
            VersionAnnotatedModData::V0_1_0(md) => md,
        }
    }
}

impl DerefMut for VersionAnnotatedModData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            VersionAnnotatedModData::V0_1_0(md) => md,
        }
    }
}

pub struct State {
    pub project_dirs: ProjectDirs,
    pub config: ConfigWrapper<VersionAnnotatedConfig>,
    pub mod_data: ConfigWrapper<VersionAnnotatedModData>,
    pub store: Arc<ModStore>,
}

impl State {
    pub fn init() -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "drg-mod-integration")
            .context("constructing project dirs")?;
        std::fs::create_dir_all(project_dirs.cache_dir())?;
        std::fs::create_dir_all(project_dirs.config_dir())?;
        let config_path = project_dirs.config_dir().join("config.json");

        let config = if config_path.exists() {
            let config: MaybeVersionedConfig = std::fs::read(&config_path)
                .ok()
                .and_then(|s| serde_json::from_slice(&s).ok())
                .unwrap_or_default();
            let config = match config {
                MaybeVersionedConfig::Versioned(v) => v,
                MaybeVersionedConfig::Legacy(legacy) => {
                    VersionAnnotatedConfig::V0_0_0(Config_v0_0_0 {
                        provider_parameters: legacy.provider_parameters,
                        drg_pak_path: legacy.drg_pak_path,
                    })
                }
            };
            ConfigWrapper::<VersionAnnotatedConfig>::new(&config_path, config)
        } else {
            ConfigWrapper::<VersionAnnotatedConfig>::new(&config_path, Default::default())
        };
        config.save().unwrap();

        let legacy_mod_profiles_path = project_dirs.config_dir().join("profiles.json");
        let mod_data_path = project_dirs.config_dir().join("mod_data.json");
        let mod_data: MaybeVersionedModData = if mod_data_path.exists() {
            std::fs::read(&mod_data_path)
                .ok()
                .and_then(|s| serde_json::from_slice(&s).ok())
                .unwrap_or_default()
        } else if legacy_mod_profiles_path.exists() {
            let mod_data = std::fs::read(&legacy_mod_profiles_path)
                .ok()
                .and_then(|s| serde_json::from_slice(&s).ok())
                .unwrap_or_default();
            let _ = std::fs::remove_file(&legacy_mod_profiles_path);
            mod_data
        } else {
            MaybeVersionedModData::default()
        };

        let mod_data = match mod_data {
            MaybeVersionedModData::Legacy(legacy) => VersionAnnotatedModData::V0_1_0(legacy.into()),
            MaybeVersionedModData::Versioned(v) => v,
        };
        let mod_data = ConfigWrapper::<VersionAnnotatedModData>::new(mod_data_path, mod_data);
        mod_data.save().unwrap();

        let store = ModStore::new(project_dirs.cache_dir(), &config.provider_parameters)?.into();
        Ok(Self {
            project_dirs,
            config,
            mod_data,
            store,
        })
    }
}
