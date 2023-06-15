pub mod error;
pub mod gui;
pub mod integrate;
pub mod providers;
pub mod state;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::Result;

use error::IntegrationError;
use providers::{ModResolution, ModSpecification, ProviderFactory, ResolvableStatus};
use state::State;

pub fn find_drg() -> Option<PathBuf> {
    steamlocate::SteamDir::locate()
        .and_then(|mut steamdir| steamdir.app(&548430).map(|a| a.path.to_path_buf()))
}

pub async fn resolve_and_integrate<P: AsRef<Path>>(
    path_game: P,
    state: &State,
    mod_specs: &[ModSpecification],
    update: bool,
) -> Result<()> {
    let mods = state.store.resolve_mods(mod_specs, update).await?;

    println!("resolvable mods:");
    for m in mod_specs {
        if let ResolvableStatus::Resolvable(resolution) = &mods[m].status {
            println!("{:?}", resolution);
        }
    }

    let mods_set = mod_specs
        .iter()
        .flat_map(|m| match &mods[m].status {
            ResolvableStatus::Resolvable(ModResolution { url }) => Some(url),
            _ => None,
        })
        .collect::<HashSet<_>>();

    let missing_deps = mod_specs
        .iter()
        .flat_map(|m| {
            mods[m]
                .suggested_dependencies
                .iter()
                .filter_map(|m| match &mods[m].status {
                    ResolvableStatus::Resolvable(ModResolution { url }) => {
                        (!mods_set.contains(url)).then_some(url)
                    }
                    _ => Some(&m.url),
                })
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
        .map(|m| &m.spec) // TODO this should be a ModResolution not a ModSpecification, we're missing a step here
        .collect::<Vec<&ModSpecification>>();

    println!("fetching mods...");
    let paths = state.store.fetch_mods(&urls, update).await?;

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
    F: Fn(&mut State, ModSpecification, &ProviderFactory) -> Result<()>,
{
    loop {
        match resolve_and_integrate(&path_game, &state, &mod_specs, update).await {
            Ok(()) => return Ok(()),
            Err(e) => match e.downcast::<IntegrationError>() {
                Ok(IntegrationError::NoProvider { spec, factory }) => init(state, spec, factory)?,
                Err(e) => return Err(e),
            },
        }
    }
}
