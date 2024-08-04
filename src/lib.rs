#![feature(let_chains)]
#![feature(if_let_guard)]

pub mod gui;
pub mod integrate;
pub mod mod_lints;
pub mod providers;
pub mod state;

use std::ops::Deref;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use fs_err as fs;
use integrate::IntegrationError;
use providers::{ModResolution, ModSpecification, ProviderError, ProviderFactory};
use thiserror::Error;
use tracing::*;

use state::{State, StateError};

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    RepakError(#[from] repak::Error),
    #[error(transparent)]
    ProviderError(#[from] ProviderError),
    #[error(transparent)]
    IntegrationError(#[from] IntegrationError),
    #[error(transparent)]
    MintError(#[from] mint_lib::MintError),
    #[error(transparent)]
    StateError(#[from] StateError),
    #[error("invalid DRG pak path: {}", path.display())]
    InvalidDrgPak { path: PathBuf },
    #[error("{0}")]
    GenericError(String),
}

#[derive(Debug)]
pub struct Dirs {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Dirs {
    pub fn default_xdg() -> Result<Self, AppError> {
        let legacy_dirs = ProjectDirs::from("", "", "drg-mod-integration")
            .expect("failed to construct project dirs");

        let project_dirs =
            ProjectDirs::from("", "", "mint").expect("failed to construct project dirs");

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

    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, AppError> {
        Self::from_paths(
            path.as_ref().join("config"),
            path.as_ref().join("cache"),
            path.as_ref().join("data"),
        )
    }

    fn from_paths<P: AsRef<Path>>(
        config_dir: P,
        cache_dir: P,
        data_dir: P,
    ) -> Result<Self, AppError> {
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

pub fn is_drg_pak<P: AsRef<Path>>(path: P) -> Result<(), AppError> {
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
) -> Result<(), IntegrationError> {
    let mods = state.store.resolve_mods(mod_specs, update).await?;

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
        .map(|m| &m.resolution)
        .collect::<Vec<_>>();

    info!("fetching mods...");
    let paths = state.store.fetch_mods(&urls, update, None).await?;

    integrate::integrate(
        game_path,
        state.config.deref().into(),
        to_integrate.into_iter().zip(paths).collect(),
    )
}

async fn resolve_into_urls<'b>(
    state: &State,
    mod_specs: &[ModSpecification],
) -> Result<Vec<ModResolution>, AppError> {
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
) -> Result<Vec<PathBuf>, AppError> {
    let urls = resolve_into_urls(state, mod_specs).await?;
    Ok(state
        .store
        .fetch_mods(&urls.iter().collect::<Vec<_>>(), false, None)
        .await?)
}

pub async fn resolve_unordered_and_integrate_with_provider_init<P, F>(
    game_path: P,
    state: &mut State,
    mod_specs: &[ModSpecification],
    update: bool,
    init: F,
) -> Result<(), AppError>
where
    P: AsRef<Path>,
    F: Fn(&mut State, String, &ProviderFactory) -> Result<(), AppError>,
{
    loop {
        match resolve_unordered_and_integrate(&game_path, state, mod_specs, update).await {
            Ok(()) => return Ok(()),
            Err(ref e)
                if let IntegrationError::ProviderError(src) = e
                    && let ProviderError::NoProvider { ref url, factory } = src =>
            {
                init(state, url.clone(), factory)?
            }
            Err(e) => Err(e)?,
        }
    }
}

#[allow(clippy::needless_pass_by_ref_mut)]
pub async fn resolve_ordered_with_provider_init<F>(
    state: &mut State,
    mod_specs: &[ModSpecification],
    init: F,
) -> Result<Vec<PathBuf>, AppError>
where
    F: Fn(&mut State, String, &ProviderFactory) -> Result<(), AppError>,
{
    loop {
        match resolve_ordered(state, mod_specs).await {
            Ok(mod_paths) => return Ok(mod_paths),
            Err(ref e)
                if let AppError::IntegrationError(src) = e
                    && let IntegrationError::ProviderError(src) = src
                    && let ProviderError::NoProvider { ref url, factory } = src =>
            {
                init(state, url.clone(), factory)?
            }
            Err(e) => Err(e)?,
        }
    }
}
