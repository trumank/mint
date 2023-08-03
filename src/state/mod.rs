pub mod config;

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::{
    find_drg_pak,
    providers::{ModSpecification, ModStore},
};

use self::config::ConfigWrapper;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ModProfiles {
    pub active_profile: String,
    pub profiles: BTreeMap<String, ModProfile>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModProfile {
    pub mods: Vec<ModConfig>,
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

impl Default for ModProfiles {
    fn default() -> Self {
        Self {
            active_profile: "default".to_string(),
            profiles: [("default".to_string(), Default::default())]
                .into_iter()
                .collect(),
        }
    }
}

impl ModProfiles {
    pub fn get_active_profile(&self) -> &ModProfile {
        &self.profiles[&self.active_profile]
    }
    pub fn get_active_profile_mut(&mut self) -> &mut ModProfile {
        self.profiles.get_mut(&self.active_profile).unwrap()
    }
    pub fn remove_active(&mut self) {
        self.profiles.remove(&self.active_profile);
        self.active_profile = self.profiles.keys().next().unwrap().to_string();
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub provider_parameters: HashMap<String, HashMap<String, String>>,
    pub drg_pak_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider_parameters: Default::default(),
            drg_pak_path: find_drg_pak(),
        }
    }
}

pub struct State {
    pub project_dirs: ProjectDirs,
    pub config: ConfigWrapper<Config>,
    pub profiles: ConfigWrapper<ModProfiles>,
    pub store: Arc<ModStore>,
}

impl State {
    pub fn new() -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "drg-mod-integration")
            .context("constructing project dirs")?;
        std::fs::create_dir_all(project_dirs.cache_dir())?;
        std::fs::create_dir_all(project_dirs.config_dir())?;
        let config = ConfigWrapper::<Config>::new(project_dirs.config_dir().join("config.json"));
        let profiles =
            ConfigWrapper::<ModProfiles>::new(project_dirs.config_dir().join("profiles.json"));
        let store = ModStore::new(project_dirs.cache_dir(), &config.provider_parameters)?.into();
        Ok(Self {
            project_dirs,
            config,
            profiles,
            store,
        })
    }
}
