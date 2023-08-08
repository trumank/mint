#![feature(let_chains)]

pub mod error;
pub mod gui;
pub mod integrate;
pub mod providers;
pub mod splice;
pub mod state;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};

use error::IntegrationError;
use providers::{ModSpecification, ProviderFactory};
use state::State;

pub enum DRGInstallationType {
    Steam,
    Xbox,
}
impl DRGInstallationType {
    pub fn from_pak_path<P: AsRef<Path>>(pak: P) -> Result<Self> {
        let pak_name = pak
            .as_ref()
            .file_name()
            .context("failed to get pak file name")?
            .to_string_lossy()
            .to_lowercase();
        Ok(match pak_name.as_str() {
            "fsd-windowsnoeditor.pak" => Self::Steam,
            "fsd-wingdk.pak" => Self::Xbox,
            _ => bail!("unrecognized pak file name: {pak_name}"),
        })
    }
    pub fn binaries_directory_name(&self) -> &'static str {
        match self {
            Self::Steam => "Win64",
            Self::Xbox => "WinGDK",
        }
    }
    pub fn main_pak_name(&self) -> &'static str {
        match self {
            Self::Steam => "FSD-WindowsNoEditor.pak",
            Self::Xbox => "FSD-WinGDK.pak",
        }
    }
}

pub struct DRGInstallation {
    pub root: PathBuf,
    pub installation_type: DRGInstallationType,
}

impl DRGInstallation {
    /// Returns first DRG installation found. Only supports Steam version
    /// TODO locate Xbox version
    pub fn find() -> Option<Self> {
        steamlocate::SteamDir::locate()
            .and_then(|mut steamdir| {
                steamdir
                    .app(&548430)
                    .map(|a| a.path.join("FSD/Content/Paks/FSD-WindowsNoEditor.pak"))
            })
            .and_then(|path| Self::from_pak_path(path).ok())
    }
    pub fn from_pak_path<P: AsRef<Path>>(pak: P) -> Result<Self> {
        let root = pak
            .as_ref()
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("failed to get pak parent directory")?
            .to_path_buf();
        Ok(Self {
            root,
            installation_type: DRGInstallationType::from_pak_path(pak)?,
        })
    }
    pub fn binaries_directory(&self) -> PathBuf {
        self.root
            .join("Binaries")
            .join(self.installation_type.binaries_directory_name())
    }
    pub fn paks_path(&self) -> PathBuf {
        self.root.join("Content").join("Paks")
    }
    pub fn main_pak(&self) -> PathBuf {
        self.root
            .join("Content")
            .join("Paks")
            .join(self.installation_type.main_pak_name())
    }
    pub fn modio_directory(&self) -> Option<PathBuf> {
        match self.installation_type {
            DRGInstallationType::Steam => {
                #[cfg(target_os = "windows")]
                {
                    Some(PathBuf::from("C:\\Users\\Public\\mod.io\\2475"))
                }
                #[cfg(target_os = "linux")]
                {
                    steamlocate::SteamDir::locate().map(|s| {
                        s.path.join(
                            "steamapps/compatdata/548430/pfx/drive_c/users/Public/mod.io/2475",
                        )
                    })
                }
            }
            DRGInstallationType::Xbox => None,
        }
    }
}

/// File::open with the file path included in any error messages
pub fn open_file<P: AsRef<Path>>(path: P) -> Result<std::fs::File> {
    std::fs::File::open(&path)
        .with_context(|| format!("Could not open file {}", path.as_ref().display()))
}

/// fs::read with the file path included in any error messages
pub fn read_file<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    std::fs::read(&path).with_context(|| format!("Could not read file {}", path.as_ref().display()))
}

/// fs::write with the file path included in any error messages
pub fn write_file<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, data: C) -> Result<()> {
    std::fs::write(&path, data)
        .with_context(|| format!("Could not write to file {}", path.as_ref().display()))
}

pub fn is_drg_pak<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut reader = std::io::BufReader::new(open_file(path)?);
    let pak = repak::PakReader::new_any(&mut reader, None)?;
    pak.get("FSD/FSD.uproject", &mut reader)?;
    Ok(())
}

pub async fn resolve_and_integrate<P: AsRef<Path>>(
    path_game: P,
    state: &State,
    mod_specs: &[ModSpecification],
    update: bool,
) -> Result<()> {
    let mods = state.store.resolve_mods(mod_specs, update).await?;

    let mods_set = mod_specs
        .iter()
        .flat_map(|m| [&mods[m].spec.url, &mods[m].resolution.url])
        .collect::<HashSet<_>>();

    // TODO need more rebust way of detecting whether dependencies are missing
    let missing_deps = mod_specs
        .iter()
        .flat_map(|m| {
            mods[m]
                .suggested_dependencies
                .iter()
                .filter_map(|m| (!mods_set.contains(&m.url)).then_some(&m.url))
        })
        .collect::<HashSet<_>>();
    if !missing_deps.is_empty() {
        println!("WARNING: The following dependencies are missing:");
        for d in missing_deps {
            println!("  {d}");
        }
    }

    let to_integrate = mod_specs
        .iter()
        .map(|u| mods[u].clone())
        .collect::<Vec<_>>();
    let urls = to_integrate
        .iter()
        .map(|m| &m.resolution)
        .collect::<Vec<_>>();

    println!("fetching mods...");
    let paths = state.store.fetch_mods(&urls, update, None).await?;

    integrate::integrate(path_game, to_integrate.into_iter().zip(paths).collect())
}

pub async fn resolve_and_integrate_with_provider_init<P, F>(
    path_game: P,
    state: &mut State,
    mod_specs: &[ModSpecification],
    update: bool,
    init: F,
) -> Result<()>
where
    P: AsRef<Path>,
    F: Fn(&mut State, String, &ProviderFactory) -> Result<()>,
{
    loop {
        match resolve_and_integrate(&path_game, state, mod_specs, update).await {
            Ok(()) => return Ok(()),
            Err(e) => match e.downcast::<IntegrationError>() {
                Ok(IntegrationError::NoProvider { url, factory }) => init(state, url, factory)?,
                Err(e) => return Err(e),
            },
        }
    }
}
