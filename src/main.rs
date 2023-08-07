use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use drg_mod_integration::{
    gui::gui,
    providers::{ModSpecification, ProviderFactory},
    resolve_and_integrate_with_provider_init,
    state::State,
    DRGInstallation,
};

/// Command line integration tool.
#[derive(Parser, Debug)]
struct ActionIntegrate {
    /// Path to FSD-WindowsNoEditor.pak (FSD-WinGDK.pak for Microsoft Store version) located
    /// inside the "Deep Rock Galactic" installation directory under FSD/Content/Paks. Only
    /// necessary if it cannot be found automatically.
    #[arg(short, long)]
    fsd_pak: Option<PathBuf>,

    /// Update mods. By default all mods and metadata are cached offline so this is necessary to
    /// check for updates.
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

/// Integrate a mod group
#[derive(Parser, Debug)]
struct ActionIntegrateProfile {
    /// Path to FSD-WindowsNoEditor.pak (FSD-WinGDK.pak for Microsoft Store version) located
    /// inside the "Deep Rock Galactic" installation directory under FSD/Content/Paks. Only
    /// necessary if it cannot be found automatically.
    #[arg(short, long)]
    fsd_pak: Option<PathBuf>,

    /// Update mods. By default all mods and metadata are cached offline so this is necessary to
    /// check for updates.
    #[arg(short, long)]
    update: bool,

    /// Profile to integrate.
    profile: String,
}

/// Launch via steam
#[derive(Parser, Debug)]
struct ActionLaunch {
    args: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Action {
    Integrate(ActionIntegrate),
    Profile(ActionIntegrateProfile),
    Launch(ActionLaunch),
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    #[command(subcommand)]
    action: Option<Action>,
}

fn main() -> Result<()> {
    std::env::set_var("RUST_BACKTRACE", "1");
    let rt = tokio::runtime::Runtime::new().expect("Unable to create Runtime");
    let _enter = rt.enter();

    let args = Args::parse();

    match args.action {
        Some(Action::Integrate(action)) => rt.block_on(async {
            action_integrate(action).await?;
            Ok(())
        }),
        Some(Action::Profile(action)) => rt.block_on(async {
            action_integrate_profile(action).await?;
            Ok(())
        }),
        Some(Action::Launch(action)) => {
            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });
            gui(Some(action.args))?;
            Ok(())
        }
        None => {
            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });
            gui(None)?;
            Ok(())
        }
    }
}

fn init_provider(state: &mut State, url: String, factory: &ProviderFactory) -> Result<()> {
    println!("Initializing provider for {:?}", url);
    let params = state
        .config
        .provider_parameters
        .entry(factory.id.to_owned())
        .or_default();
    for p in factory.parameters {
        if !params.contains_key(p.name) {
            // this blocks but since we're calling it on the main thread it'll be fine
            let value =
                dialoguer::Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt(p.description)
                    .interact()
                    .unwrap();
            params.insert(p.id.to_owned(), value);
        }
    }
    state.store.add_provider(factory, params)
}

async fn action_integrate(action: ActionIntegrate) -> Result<()> {
    let path_game_pak = action
        .fsd_pak
        .or_else(|| {
            DRGInstallation::find()
                .as_ref()
                .map(DRGInstallation::main_pak)
        })
        .context("Could not find DRG pak file, please specify manually with the --fsd_pak flag")?;

    let mut state = State::init()?;

    let mod_specs = action
        .mods
        .into_iter()
        .map(ModSpecification::new)
        .collect::<Vec<_>>();

    resolve_and_integrate_with_provider_init(
        path_game_pak,
        &mut state,
        &mod_specs,
        action.update,
        init_provider,
    )
    .await
}

async fn action_integrate_profile(action: ActionIntegrateProfile) -> Result<()> {
    let path_game_pak = action
        .fsd_pak
        .or_else(|| {
            DRGInstallation::find()
                .as_ref()
                .map(DRGInstallation::main_pak)
        })
        .context("Could not find DRG pak file, please specify manually with the --fsd_pak flag")?;

    let mut state = State::init()?;

    let mut mod_specs = Vec::new();
    for (name, enabled) in &state.mod_data.profiles[&action.profile].mod_groups {
        if *enabled {
            for m in &state.mod_data.groups[name].mods {
                if m.enabled {
                    mod_specs.push(m.spec.clone());
                }
            }
        }
    }

    resolve_and_integrate_with_provider_init(
        path_game_pak,
        &mut state,
        &mod_specs,
        action.update,
        init_provider,
    )
    .await
}
