use std::collections::HashMap;

use crate::providers::ModSpecification;

pub mod config;

/// Mod configuration, holds ModSpecification as well as other metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModConfig {
    pub spec: ModSpecification,
    pub required: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ModProfiles {
    pub active_profile: String,
    pub profiles: HashMap<String, ModProfile>,
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

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModProfile {
    pub mods: Vec<ModConfig>,
}
