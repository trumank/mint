use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use tracing::{debug, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter;

use drg_mod_integration::mod_lint::lint;
use drg_mod_integration::providers::ProviderFactory;
use drg_mod_integration::{gui::gui, providers::ModSpecification, state::State, DRGInstallation};
use drg_mod_integration::{
    resolve_ordered_with_provider_init, resolve_unordered_and_integrate_with_provider_init,
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

/// Integrate a profile
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

/// Lint the mod bundle that would be created for a profile.
#[derive(Parser, Debug)]
struct ActionLint {
    /// Path to FSD-WindowsNoEditor.pak (FSD-WinGDK.pak for Microsoft Store version) located
    /// inside the "Deep Rock Galactic" installation directory under FSD/Content/Paks. Only
    /// necessary if it cannot be found automatically.
    #[arg(short, long)]
    fsd_pak: Option<PathBuf>,

    /// Profile to lint.
    profile: String,
}

#[derive(Subcommand, Debug)]
enum Action {
    Integrate(ActionIntegrate),
    Profile(ActionIntegrateProfile),
    Launch(ActionLaunch),
    Lint(ActionLint),
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    #[command(subcommand)]
    action: Option<Action>,
}

fn main() -> Result<()> {
    std::env::set_var("RUST_BACKTRACE", "1");
    let _guard = setup_logging()?;
    debug!("logging setup complete");

    let rt = tokio::runtime::Runtime::new().expect("Unable to create Runtime");
    debug!("tokio runtime created");
    let _enter = rt.enter();

    let args = Args::parse();
    debug!(?args);

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
        Some(Action::Lint(action)) => rt.block_on(async {
            action_lint(action).await?;
            Ok(())
        }),
        None => {
            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });
            gui(None)?;
            Ok(())
        }
    }
}

fn setup_logging() -> Result<WorkerGuard> {
    use tracing::metadata::LevelFilter;
    use tracing::Level;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{
        field::RecordFields,
        fmt::{
            self,
            format::{Pretty, Writer},
            FormatFields,
        },
        EnvFilter,
    };

    /// Workaround for <https://github.com/tokio-rs/tracing/issues/1817>.
    struct NewType(Pretty);

    impl<'writer> FormatFields<'writer> for NewType {
        fn format_fields<R: RecordFields>(
            &self,
            writer: Writer<'writer>,
            fields: R,
        ) -> core::fmt::Result {
            self.0.format_fields(writer, fields)
        }
    }

    let project_dirs =
        ProjectDirs::from("", "", "drg-mod-integration").context("constructing project dirs")?;
    std::fs::create_dir_all(project_dirs.data_dir())?;

    let f = File::create(project_dirs.data_dir().join("drg-mod-integration.log"))?;
    let writer = BufWriter::new(f);
    let (log_file_appender, guard) = tracing_appender::non_blocking(writer);
    let debug_file_log = fmt::layer()
        .with_writer(log_file_appender)
        .fmt_fields(NewType(Pretty::default()))
        .with_ansi(false)
        .with_filter(filter::Targets::new().with_target("drg_mod_integration", Level::DEBUG));
    let stderr_log = fmt::layer()
        .with_writer(std::io::stderr)
        .compact()
        .with_level(true)
        .with_target(true)
        .without_time()
        .with_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        );
    let subscriber = tracing_subscriber::registry()
        .with(stderr_log)
        .with(debug_file_log);

    tracing::subscriber::set_global_default(subscriber)?;

    debug!("tracing subscriber setup");
    info!(
        "writing logs to `{}`",
        project_dirs
            .data_dir()
            .join("drg-mod-integration.log")
            .display()
    );

    Ok(guard)
}

#[tracing::instrument(skip(state))]
fn init_provider(state: &mut State, url: String, factory: &ProviderFactory) -> Result<()> {
    info!("initializing provider for {:?}", url);

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
    debug!(?path_game_pak);

    let mut state = State::init()?;

    let mod_specs = action
        .mods
        .into_iter()
        .map(ModSpecification::new)
        .collect::<Vec<_>>();

    resolve_unordered_and_integrate_with_provider_init(
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
    debug!(?path_game_pak);

    let mut state = State::init()?;

    let mut mods = Vec::new();
    state.mod_data.for_each_enabled_mod(&action.profile, |mc| {
        mods.push(mc.spec.clone());
    });

    resolve_unordered_and_integrate_with_provider_init(
        path_game_pak,
        &mut state,
        &mods,
        action.update,
        init_provider,
    )
    .await
}

async fn action_lint(action: ActionLint) -> Result<()> {
    let path_game_pak = action
        .fsd_pak
        .or_else(|| {
            DRGInstallation::find()
                .as_ref()
                .map(DRGInstallation::main_pak)
        })
        .context("Could not find DRG pak file, please specify manually with the --fsd_pak flag")?;
    debug!(?path_game_pak);

    let mut state = State::init()?;

    let mut mods = Vec::new();
    state.mod_data.for_each_mod(&action.profile, |mc| {
        mods.push(mc.spec.clone());
    });

    let mod_paths = resolve_ordered_with_provider_init(&mut state, &mods, init_provider).await?;

    let mods = mods.into_iter().zip(mod_paths).collect::<Vec<_>>();

    let report = tokio::task::spawn_blocking(move || lint(&path_game_pak, &mods)).await??;
    println!("{:#?}", report);
    Ok(())
}
