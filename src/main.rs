mod config;
mod error;
mod gui;
mod integrate;
mod providers;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use gui::gui;
use serde::{Deserialize, Serialize};

use config::ConfigWrapper;
use error::IntegrationError;
use providers::ResolvableStatus;

use crate::providers::ModResolution;

#[derive(Parser, Debug)]
struct ActionIntegrate {
    /// Path to the "Deep Rock Galactic" installation directory
    #[arg(short, long)]
    drg: Option<PathBuf>,

    /// Update mods. By default only offline cached data will be used without this flag.
    #[arg(short, long)]
    update: bool,

    /// Path of mods to integrate
    #[arg(short, long, num_args=0..)]
    mods: Vec<String>,
}

#[derive(Parser, Debug)]
struct ActionGui {}

#[derive(Subcommand, Debug)]
enum Action {
    Integrate(ActionIntegrate),
    Gui(ActionGui),
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();

    let args = Args::parse();

    match args.action {
        Action::Integrate(action) => rt.block_on(async {
            action_integrate(action).await?;
            Ok(())
        }),
        Action::Gui(action) => {
            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });
            action_gui(action)?;
            Ok(())
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    provider_parameters: HashMap<String, HashMap<String, String>>,
}

async fn action_integrate(action: ActionIntegrate) -> Result<()> {
    let path_game = action
        .drg
        .or_else(|| {
            if let Some(mut steamdir) = steamlocate::SteamDir::locate() {
                steamdir.app(&548430).map(|a| a.path.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            anyhow!(
                "Could not find DRG install directory, please specify manually with the --drg flag"
            )
        })?;

    let data_dir = Path::new("data");

    std::fs::create_dir(data_dir).ok();
    let mut config: ConfigWrapper<Config> = ConfigWrapper::new(data_dir.join("config.json"));
    let mut store = providers::ModStore::new(data_dir, &config.provider_parameters)?;

    let mods = loop {
        match store.resolve_mods(&action.mods, action.update).await {
            Ok(mods) => break mods,
            Err(e) => match e.downcast::<IntegrationError>() {
                Ok(IntegrationError::NoProvider { url, factory }) => {
                    println!("Initializing provider for {url}");
                    let params = config
                        .provider_parameters
                        .entry(factory.id.to_owned())
                        .or_default();
                    for p in factory.parameters {
                        if !params.contains_key(p.name) {
                            let value = dialoguer::Password::with_theme(
                                &dialoguer::theme::ColorfulTheme::default(),
                            )
                            .with_prompt(p.description)
                            .interact()
                            .unwrap();
                            params.insert(p.id.to_owned(), value);
                        }
                    }
                    store.add_provider(factory, params)?;
                }
                Err(e) => return Err(e),
            },
        }
    };

    println!("resolvable mods:");
    for m in &action.mods {
        if let ResolvableStatus::Resolvable(ModResolution { url }) = &mods[m].status {
            println!("{url}");
        }
    }

    let mods_set = action
        .mods
        .iter()
        .flat_map(|m| match &mods[m].status {
            ResolvableStatus::Resolvable(ModResolution { url }) => Some(url),
            _ => None,
        })
        .collect::<HashSet<_>>();

    let missing_deps = action
        .mods
        .iter()
        .flat_map(|m| {
            mods[m]
                .suggested_dependencies
                .iter()
                .filter_map(|m| match &mods[m].status {
                    ResolvableStatus::Resolvable(ModResolution { url }) => {
                        (!mods_set.contains(url)).then_some(url)
                    }
                    _ => Some(m),
                })
        })
        .collect::<HashSet<_>>();
    if !missing_deps.is_empty() {
        println!("WARNING: The following dependencies are missing:");
        for d in missing_deps {
            println!("  {d}");
        }
    }

    let to_integrate = action
        .mods
        .iter()
        .map(|u| mods[u].clone())
        .collect::<Vec<_>>();
    let urls = to_integrate
        .iter()
        .map(|m| m.url.as_ref())
        .collect::<Vec<&str>>();

    println!("fetching mods...");
    let paths = store.fetch_mods(&urls, action.update).await?;

    integrate::integrate(path_game, to_integrate.into_iter().zip(paths).collect())
}

fn action_gui(_action: ActionGui) -> Result<()> {
    gui()
}
