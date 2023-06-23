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
use providers::{ModSpecification, ProviderFactory};
use state::State;

pub fn find_drg_pak() -> Option<PathBuf> {
    steamlocate::SteamDir::locate().and_then(|mut steamdir| {
        steamdir
            .app(&548430)
            .map(|a| a.path.join("FSD/Content/Paks/FSD-WindowsNoEditor.pak"))
    })
}

pub fn is_drg_pak<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
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
