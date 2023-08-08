pub mod config;

use std::{
    collections::{BTreeMap, HashMap},
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::{
    providers::{ModSpecification, ModStore},
    DRGInstallation,
};

use self::config::ConfigWrapper;

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

#[obake::versioned]
#[obake(version("0.0.0"))]
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModProfile {
    #[obake(cfg("0.0.0"))]
    pub mods: Vec<ModConfig>,
}

#[obake::versioned]
#[obake(version("0.0.0"))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModData {
    pub active_profile: String,
    #[obake(cfg("0.0.0"))]
    pub profiles: BTreeMap<String, ModProfile!["0.0.0"]>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version")]
pub enum VersionAnnotatedModData {
    #[serde(rename = "0.0.0")]
    V0_0_0(ModData!["0.0.0"]),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum MaybeVersionedModData {
    Versioned(VersionAnnotatedModData),
    Legacy(ModData!["0.0.0"]),
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

impl Default for MaybeVersionedModData {
    fn default() -> Self {
        MaybeVersionedModData::Versioned(Default::default())
    }
}

impl Default for VersionAnnotatedModData {
    fn default() -> Self {
        VersionAnnotatedModData::V0_0_0(Default::default())
    }
}

impl Deref for VersionAnnotatedModData {
    type Target = ModData!["0.0.0"];

    fn deref(&self) -> &Self::Target {
        match self {
            VersionAnnotatedModData::V0_0_0(md) => md,
        }
    }
}

impl DerefMut for VersionAnnotatedModData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            VersionAnnotatedModData::V0_0_0(md) => md,
        }
    }
}

impl ModData!["0.0.0"] {
    pub fn get_active_profile(&self) -> &ModProfile {
        &self.profiles[&self.active_profile]
    }

    pub fn get_active_profile_mut(&mut self) -> &mut ModProfile {
        self.profiles.get_mut(&self.active_profile).unwrap()
    }

    pub fn remove_active_profile(&mut self) {
        self.profiles.remove(&self.active_profile);
        self.active_profile = self.profiles.keys().next().unwrap().to_string();
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
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum MaybeVersionedConfig {
    Versioned(VersionAnnotatedConfig),
    Legacy(Config!["0.0.0"]),
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
            VersionAnnotatedConfig::Unsupported => unreachable!(),
        }
    }
}

impl DerefMut for VersionAnnotatedConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            VersionAnnotatedConfig::V0_0_0(cfg) => cfg,
            VersionAnnotatedConfig::Unsupported => unreachable!(),
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

        let config = read_config_or_default(&config_path)?;
        let config = ConfigWrapper::<VersionAnnotatedConfig>::new(&config_path, config);
        config.save().unwrap();

        let legacy_mod_profiles_path = project_dirs.config_dir().join("profiles.json");
        let mod_data_path = project_dirs.config_dir().join("mod_data.json");
        let mod_data = read_mod_data_or_default(&mod_data_path, legacy_mod_profiles_path)?;
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

fn read_config_or_default(config_path: &PathBuf) -> Result<VersionAnnotatedConfig> {
    Ok(match std::fs::read(config_path) {
        Ok(buf) => {
            let config = serde_json::from_slice::<MaybeVersionedConfig>(&buf)
                .context("failed to deserialize user config into maybe versioned config")?;
            match config {
                MaybeVersionedConfig::Versioned(v) => v,
                MaybeVersionedConfig::Legacy(legacy) => {
                    VersionAnnotatedConfig::V0_0_0(Config_v0_0_0 {
                        provider_parameters: legacy.provider_parameters,
                        drg_pak_path: legacy.drg_pak_path,
                    })
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => VersionAnnotatedConfig::default(),
        Err(e) => Err(e).context("failed to read `config.json`")?,
    })
}

fn read_mod_data_or_default(
    mod_data_path: &PathBuf,
    legacy_mod_profiles_path: PathBuf,
) -> Result<VersionAnnotatedModData> {
    let mod_data = match std::fs::read(mod_data_path) {
        Ok(buf) => serde_json::from_slice::<MaybeVersionedModData>(&buf)
            .context("failed to deserialize existing `mod_data.json`")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::read(&legacy_mod_profiles_path) {
                Ok(buf) => {
                    let mod_data = serde_json::from_slice::<MaybeVersionedModData>(&buf)
                        .context("failed to deserialize legacy `profiles.json`")?;
                    std::fs::remove_file(&legacy_mod_profiles_path)
                        .context("failed to remove legacy `profiles.json` while migrating")?;
                    mod_data
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    MaybeVersionedModData::default()
                }
                Err(e) => Err(e).context("failed to read legacy `profiles`.json")?,
            }
        }
        Err(e) => Err(e).context("failed to read `mod_data`.json")?,
    };

    let mod_data = match mod_data {
        MaybeVersionedModData::Legacy(legacy) => VersionAnnotatedModData::V0_0_0(legacy),
        MaybeVersionedModData::Versioned(v) => v,
    };

    Ok(mod_data)
}
