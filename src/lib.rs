#![feature(let_chains)]

pub mod error;
pub mod gui;
pub mod integrate;
pub mod mod_lints;
pub mod providers;
pub mod state;

use std::io::{Cursor, Read};
use std::str::FromStr;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};

use directories::ProjectDirs;
use error::IntegrationError;
use integrate::IntegrationErr;
use providers::{ModResolution, ModSpecification, ProviderFactory, ReadSeek};
use state::State;
use tracing::{info, warn};

pub struct Dirs {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Dirs {
    pub fn defauld_xdg() -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "drg-mod-integration")
            .context("constructing project dirs")?;

        Self::from_paths(
            project_dirs.config_dir(),
            project_dirs.cache_dir(),
            project_dirs.data_dir(),
        )
    }
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_paths(
            path.as_ref().join("config"),
            path.as_ref().join("cache"),
            path.as_ref().join("data"),
        )
    }
    fn from_paths<P: AsRef<Path>>(config_dir: P, cache_dir: P, data_dir: P) -> Result<Self> {
        std::fs::create_dir_all(&config_dir)?;
        std::fs::create_dir_all(&cache_dir)?;
        std::fs::create_dir_all(&data_dir)?;

        Ok(Self {
            config_dir: config_dir.as_ref().to_path_buf(),
            cache_dir: cache_dir.as_ref().to_path_buf(),
            data_dir: data_dir.as_ref().to_path_buf(),
        })
    }
}

#[derive(Debug)]
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
    pub fn hook_dll_name(&self) -> &'static str {
        match self {
            Self::Steam => "x3daudio1_7.dll",
            Self::Xbox => "d3d9.dll",
        }
    }
}

#[derive(Debug)]
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
    let pak = repak::PakReader::new_any(&mut reader)?;
    pak.get("FSD/FSD.uproject", &mut reader)?;
    Ok(())
}

pub async fn resolve_unordered_and_integrate<P: AsRef<Path>>(
    game_path: P,
    state: &State,
    mod_specs: &[ModSpecification],
    update: bool,
) -> Result<(), IntegrationErr> {
    let mods = state
        .store
        .resolve_mods(mod_specs, update)
        .await
        .map_err(|e| IntegrationErr {
            mod_ctxt: None,
            kind: integrate::IntegrationErrKind::Generic(e),
        })?;

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
        warn!("the following dependencies are missing:");
        for d in missing_deps {
            warn!("  {d}");
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

    info!("fetching mods...");
    let paths = state
        .store
        .fetch_mods(&urls, update, None)
        .await
        .map_err(|e| IntegrationErr {
            mod_ctxt: None,
            kind: integrate::IntegrationErrKind::Generic(e),
        })?;

    integrate::integrate(game_path, to_integrate.into_iter().zip(paths).collect())
}

async fn resolve_into_urls<'b>(
    state: &State,
    mod_specs: &[ModSpecification],
) -> Result<Vec<ModResolution>> {
    let mods = state.store.resolve_mods(mod_specs, false).await?;

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
        warn!("the following dependencies are missing:");
        for d in missing_deps {
            warn!("  {d}");
        }
    }

    let urls = mod_specs
        .iter()
        .map(|u| mods[u].clone())
        .map(|m| m.resolution)
        .collect::<Vec<_>>();

    Ok(urls)
}

pub async fn resolve_ordered(
    state: &State,
    mod_specs: &[ModSpecification],
) -> Result<Vec<PathBuf>> {
    let urls = resolve_into_urls(state, mod_specs).await?;
    let urls = urls.iter().collect::<Vec<_>>();
    state.store.fetch_mods(&urls, false, None).await
}

pub async fn resolve_unordered_and_integrate_with_provider_init<P, F>(
    game_path: P,
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
        match resolve_unordered_and_integrate(&game_path, state, mod_specs, update).await {
            Ok(()) => return Ok(()),
            Err(IntegrationErr { mod_ctxt, kind }) => match kind {
                integrate::IntegrationErrKind::Generic(e) => match e.downcast::<IntegrationError>()
                {
                    Ok(IntegrationError::NoProvider { url, factory }) => init(state, url, factory)?,
                    Err(e) => {
                        return Err(if let Some(mod_ctxt) = mod_ctxt {
                            e.context(format!("while working with mod `{:?}`", mod_ctxt))
                        } else {
                            e
                        })
                    }
                },
                integrate::IntegrationErrKind::Repak(e) => {
                    return Err(if let Some(mod_ctxt) = mod_ctxt {
                        anyhow::Error::from(e)
                            .context(format!("while working with mod `{:?}`", mod_ctxt))
                    } else {
                        e.into()
                    })
                }
                integrate::IntegrationErrKind::UnrealAsset(e) => {
                    return Err(if let Some(mod_ctxt) = mod_ctxt {
                        anyhow::Error::from(e)
                            .context(format!("while working with mod `{:?}`", mod_ctxt))
                    } else {
                        e.into()
                    })
                }
            },
        }
    }
}

#[allow(clippy::needless_pass_by_ref_mut)]
pub async fn resolve_ordered_with_provider_init<F>(
    state: &mut State,
    mod_specs: &[ModSpecification],
    init: F,
) -> Result<Vec<PathBuf>>
where
    F: Fn(&mut State, String, &ProviderFactory) -> Result<()>,
{
    loop {
        match resolve_ordered(state, mod_specs).await {
            Ok(mod_paths) => return Ok(mod_paths),
            Err(e) => match e.downcast::<IntegrationError>() {
                Ok(IntegrationError::NoProvider { url, factory }) => init(state, url, factory)?,
                Err(e) => return Err(e),
            },
        }
    }
}

pub(crate) fn get_pak_from_data(mut data: Box<dyn ReadSeek>) -> Result<Box<dyn ReadSeek>> {
    if let Ok(mut archive) = zip::ZipArchive::new(&mut data) {
        (0..archive.len())
            .map(|i| -> Result<Option<Box<dyn ReadSeek>>> {
                let mut file = archive.by_index(i)?;
                match file.enclosed_name() {
                    Some(p) => {
                        if file.is_file() && p.extension().filter(|e| e == &"pak").is_some() {
                            let mut buf = vec![];
                            file.read_to_end(&mut buf)?;
                            Ok(Some(Box::new(Cursor::new(buf))))
                        } else {
                            Ok(None)
                        }
                    }
                    None => Ok(None),
                }
            })
            .find_map(Result::transpose)
            .context("zip does not contain pak")?
    } else {
        data.rewind()?;
        Ok(data)
    }
}

pub(crate) enum PakOrNotPak {
    Pak(Box<dyn ReadSeek>),
    NotPak(Box<dyn ReadSeek>),
}

pub(crate) enum GetAllFilesFromDataError {
    EmptyArchive,
    OnlyNonPakFiles,
    Other(anyhow::Error),
}

pub(crate) fn lint_get_all_files_from_data(
    mut data: Box<dyn ReadSeek>,
) -> Result<Vec<(PathBuf, PakOrNotPak)>, GetAllFilesFromDataError> {
    if let Ok(mut archive) = zip::ZipArchive::new(&mut data) {
        if archive.is_empty() {
            return Err(GetAllFilesFromDataError::EmptyArchive);
        }

        let mut files = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| GetAllFilesFromDataError::Other(e.into()))?;

            if let Some(p) = file.enclosed_name().map(Path::to_path_buf) {
                if file.is_file() {
                    if p.extension().filter(|e| e == &"pak").is_some() {
                        let mut buf = vec![];
                        file.read_to_end(&mut buf)
                            .map_err(|e| GetAllFilesFromDataError::Other(e.into()))?;
                        files.push((
                            p.to_path_buf(),
                            PakOrNotPak::Pak(Box::new(Cursor::new(buf))),
                        ));
                    } else {
                        let mut buf = vec![];
                        file.read_to_end(&mut buf)
                            .map_err(|e| GetAllFilesFromDataError::Other(e.into()))?;
                        files.push((
                            p.to_path_buf(),
                            PakOrNotPak::NotPak(Box::new(Cursor::new(buf))),
                        ));
                    }
                }
            }
        }

        if files
            .iter()
            .filter(|(_, pak_or_not_pak)| matches!(pak_or_not_pak, PakOrNotPak::Pak(..)))
            .count()
            >= 1
        {
            Ok(files)
        } else {
            Err(GetAllFilesFromDataError::OnlyNonPakFiles)
        }
    } else {
        data.rewind()
            .map_err(|e| GetAllFilesFromDataError::Other(e.into()))?;
        Ok(vec![(
            PathBuf::from_str(".").unwrap(),
            PakOrNotPak::Pak(data),
        )])
    }
}
