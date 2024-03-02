#![feature(let_chains)]

pub mod error;
pub mod gui;
pub mod integrate;
pub mod mod_lints;
pub mod providers;
pub mod state;

use std::io::{Cursor, Read};
use std::ops::Deref;
use std::str::FromStr;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use directories::ProjectDirs;
use error::IntegrationError;
use fs_err as fs;
use integrate::IntegrationErr;
use providers::{ModResolution, ModSpecification, ProviderFactory, ReadSeek};
use state::State;
use tracing::{info, warn};

#[derive(Debug)]
pub struct Dirs {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Dirs {
    pub fn default_xdg() -> Result<Self> {
        let legacy_dirs = ProjectDirs::from("", "", "drg-mod-integration")
            .context("constructing project dirs")?;

        let project_dirs =
            ProjectDirs::from("", "", "mint").context("constructing project dirs")?;

        Self::from_paths(
            Some(legacy_dirs.config_dir())
                .filter(|p| p.exists())
                .unwrap_or(project_dirs.config_dir()),
            Some(legacy_dirs.cache_dir())
                .filter(|p| p.exists())
                .unwrap_or(project_dirs.cache_dir()),
            Some(legacy_dirs.data_dir())
                .filter(|p| p.exists())
                .unwrap_or(project_dirs.data_dir()),
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
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(&cache_dir)?;
        fs::create_dir_all(&data_dir)?;

        Ok(Self {
            config_dir: config_dir.as_ref().to_path_buf(),
            cache_dir: cache_dir.as_ref().to_path_buf(),
            data_dir: data_dir.as_ref().to_path_buf(),
        })
    }
}

pub fn is_drg_pak<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut reader = std::io::BufReader::new(fs::File::open(path.as_ref())?);
    let pak = repak::PakBuilder::new().reader(&mut reader)?;
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
        .flat_map(|m| [&mods[m].spec.url, &mods[m].resolution.url.0])
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
        .map(|m| m.resolution.clone())
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

    integrate::integrate(
        game_path,
        state.config.deref().into(),
        state.store.clone(),
        to_integrate.into_iter().zip(paths).collect(),
    )
}

async fn resolve_into_urls<'b>(
    state: &State,
    mod_specs: &[ModSpecification],
) -> Result<Vec<ModResolution>> {
    let mods = state.store.resolve_mods(mod_specs, false).await?;

    let mods_set = mod_specs
        .iter()
        .flat_map(|m| [&mods[m].spec.url, &mods[m].resolution.url.0])
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
    NotPak,
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
                        files.push((p.to_path_buf(), PakOrNotPak::NotPak));
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
