use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use drg_mod_integration::{
    find_drg,
    gui::gui,
    providers::{ModSpecification, ProviderFactory},
    resolve_and_integrate_with_provider_init,
    state::State,
};

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

/// Integrate a profile
#[derive(Parser, Debug)]
struct ActionIntegrateProfile {
    /// Path to the "Deep Rock Galactic" installation directory. Only necessary if it cannot be found automatically.
    #[arg(short, long)]
    drg: Option<PathBuf>,

    /// Update mods. By default all mods and metadata are cached offline so this is necessary to check for updates.
    #[arg(short, long)]
    update: bool,

    /// Paths of mods to integrate
    profile: String,
}

/// Work in progress GUI. Not usable yet.
#[derive(Parser, Debug)]
struct ActionGui {}

#[derive(Subcommand, Debug)]
enum Action {
    Integrate(ActionIntegrate),
    Profile(ActionIntegrateProfile),
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
        Action::Profile(action) => rt.block_on(async {
            action_integrate_profile(action).await?;
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

fn init_provider(
    state: &mut State,
    spec: ModSpecification,
    factory: &ProviderFactory,
) -> Result<()> {
    println!("Initializing provider for {:?}", spec);
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
    let path_game = action.drg.or_else(find_drg).context(
        "Could not find DRG install directory, please specify manually with the --drg flag",
    )?;

    let mut state = State::new()?;

    let mod_specs = action
        .mods
        .into_iter()
        .map(|url| ModSpecification { url })
        .collect::<Vec<_>>();

    resolve_and_integrate_with_provider_init(
        path_game,
        &mut state,
        &mod_specs,
        action.update,
        init_provider,
    )
    .await
}

async fn action_integrate_profile(action: ActionIntegrateProfile) -> Result<()> {
    let path_game = action.drg.or_else(find_drg).context(
        "Could not find DRG install directory, please specify manually with the --drg flag",
    )?;

    let mut state = State::new()?;
    let profile = &state.profiles.profiles[&action.profile];

    let mod_specs = profile
        .mods
        .iter()
        .map(|config| config.spec.clone())
        .collect::<Vec<_>>();

    resolve_and_integrate_with_provider_init(
        path_game,
        &mut state,
        &mod_specs,
        action.update,
        init_provider,
    )
    .await
}

fn action_gui(_action: ActionGui) -> Result<()> {
    gui()
}
