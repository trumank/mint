mod error;
mod gui;
mod integrate;
mod providers;
mod state;

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use gui::gui;

use error::IntegrationError;
use providers::ResolvableStatus;

use crate::providers::{ModResolution, ModSpecification};
use crate::state::State;

/// Command line integration tool.
#[derive(Parser, Debug)]
struct ActionIntegrate {
    /// Path to the "Deep Rock Galactic" installation directory. Only necessary if it cannot be found automatically.
    #[arg(short, long)]
    drg: Option<PathBuf>,

    /// Update mods. By default all mods and metadata are cached offline so this is necessary to check for updates.
    #[arg(short, long)]
    update: bool,

    /// Paths of mods to integrate
    ///
    /// Can be a file path or URL to a .pak or .zip file or a URL to a mod on https://mod.io/g/drg
    /// Examples:
    ///     ./local/path/test-mod.pak
    ///     https://mod.io/g/drg/m/custom-difficulty
    ///     https://example.org/some-online-mod-repository/public-mod.zip
    #[arg(short, long, num_args=0.., verbatim_doc_comment)]
    mods: Vec<String>,
}

/// Work in progress GUI. Not usable yet.
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
        .context(
            "Could not find DRG install directory, please specify manually with the --drg flag",
        )?;

    let mut state = State::new()?;

    let mod_specs = action
        .mods
        .iter()
        .map(|url| ModSpecification {
            url: url.to_owned(),
        })
        .collect::<Vec<_>>();

    let mods = loop {
        match state.store.resolve_mods(&mod_specs, action.update).await {
            Ok(mods) => break mods,
            Err(e) => match e.downcast::<IntegrationError>() {
                Ok(IntegrationError::NoProvider { spec, factory }) => {
                    println!("Initializing provider for {:?}", spec);
                    let params = state
                        .config
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
                    state.store.add_provider(factory, params)?;
                }
                Err(e) => return Err(e),
            },
        }
    };

    println!("resolvable mods:");
    for m in &mod_specs {
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
    let paths = state.store.fetch_mods(&urls, action.update).await?;

    integrate::integrate(path_game, to_integrate.into_iter().zip(paths).collect())
}

fn action_gui(_action: ActionGui) -> Result<()> {
    gui()
}
