pub mod config;

use std::{
    collections::{BTreeMap, HashMap},
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

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ModData {
    pub active_profile: String,
    pub profiles: BTreeMap<String, ModProfile>,
    pub active_group: String,
    pub groups: BTreeMap<String, ModGroup>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModProfile {
    /// Mapping between mod groups and if they are enabled for a profile.
    pub mod_groups: IndexMap<String, bool>,
}

impl Default for ModProfile {
    fn default() -> Self {
        Self {
            mod_groups: [("default".to_string(), false)].into(),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModGroup {
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

impl Default for ModData {
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

impl ModData {
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub provider_parameters: HashMap<String, HashMap<String, String>>,
    pub drg_pak_path: Option<PathBuf>,
}

impl Default for Config {
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
    pub config: ConfigWrapper<Config>,
    pub mod_data: ConfigWrapper<ModData>,
    pub store: Arc<ModStore>,
}

impl State {
    pub fn new() -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "drg-mod-integration")
            .context("constructing project dirs")?;
        std::fs::create_dir_all(project_dirs.cache_dir())?;
        std::fs::create_dir_all(project_dirs.config_dir())?;
        let config = ConfigWrapper::<Config>::new(project_dirs.config_dir().join("config.json"));
        let mod_data =
            ConfigWrapper::<ModData>::new(project_dirs.config_dir().join("mod_data.json"));
        let store = ModStore::new(project_dirs.cache_dir(), &config.provider_parameters)?.into();
        Ok(Self {
            project_dirs,
            config,
            mod_data,
            store,
        })
    }
}
