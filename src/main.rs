#![feature(result_option_inspect)]

use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mint_lib::DRGInstallation;
use tracing::{debug, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter;

use mint::mod_lints::{run_lints, LintId};
use mint::providers::ProviderFactory;
use mint::{gui::gui, providers::ModSpecification, state::State};
use mint::{
    resolve_ordered_with_provider_init, resolve_unordered_and_integrate_with_provider_init, Dirs,
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

    /// Location to store configs and data
    #[arg(long)]
    appdata: Option<PathBuf>,
}

fn main() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        // Try to enable ANSI code support on Windows 10 for console. If it fails, then whatever
        // *shrugs*.
        let _res = ansi_term::enable_ansi_support();
    }

    let args = Args::parse();

    let dirs = args
        .appdata
        .as_ref()
        .inspect(|p| debug!("args.app_data = `{:?}`", p))
        .map(Dirs::from_path)
        .unwrap_or_else(Dirs::default_xdg)
        .inspect(|d| debug!("dirs = {:?}", d))?;

    std::env::set_var("RUST_BACKTRACE", "1");
    let _guard = setup_logging(&dirs)?;
    debug!("logging setup complete");

    info!("config dir = {}", dirs.config_dir.display());
    info!("cache dir = {}", dirs.cache_dir.display());
    info!("data dir = {}", dirs.data_dir.display());

    let rt = tokio::runtime::Runtime::new().expect("Unable to create Runtime");
    debug!("tokio runtime created");
    let _enter = rt.enter();

    debug!(?args);

    match args.action {
        Some(Action::Integrate(action)) => rt.block_on(async {
            action_integrate(dirs, action).await?;
            Ok(())
        }),
        Some(Action::Profile(action)) => rt.block_on(async {
            action_integrate_profile(dirs, action).await?;
            Ok(())
        }),
        Some(Action::Launch(action)) => {
            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });
            gui(dirs, Some(action.args))?;
            Ok(())
        }
        Some(Action::Lint(action)) => rt.block_on(async {
            action_lint(dirs, action).await?;
            Ok(())
        }),
        None => {
            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });
            gui(dirs, None)?;
            Ok(())
        }
    }
}

fn setup_logging(dirs: &Dirs) -> Result<WorkerGuard> {
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

    let log_path = dirs.data_dir.join("mint.log");
    let f = File::create(&log_path)?;
    let writer = BufWriter::new(f);
    let (log_file_appender, guard) = tracing_appender::non_blocking(writer);
    let debug_file_log = fmt::layer()
        .with_writer(log_file_appender)
        .fmt_fields(NewType(Pretty::default()))
        .with_ansi(false)
        .with_filter(filter::Targets::new().with_target("mint", Level::DEBUG));
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
    info!("writing logs to {:?}", log_path.display());

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

async fn action_integrate(dirs: Dirs, action: ActionIntegrate) -> Result<()> {
    let game_pak_path = action
        .fsd_pak
        .or_else(|| {
            DRGInstallation::find()
                .as_ref()
                .map(DRGInstallation::main_pak)
        })
        .context("Could not find DRG pak file, please specify manually with the --fsd_pak flag")?;
    debug!(?game_pak_path);

    let mut state = State::init(dirs)?;

    let mod_specs = action
        .mods
        .into_iter()
        .map(ModSpecification::new)
        .collect::<Vec<_>>();

    resolve_unordered_and_integrate_with_provider_init(
        game_pak_path,
        &mut state,
        &mod_specs,
        action.update,
        init_provider,
    )
    .await
}

async fn action_integrate_profile(dirs: Dirs, action: ActionIntegrateProfile) -> Result<()> {
    let game_pak_path = action
        .fsd_pak
        .or_else(|| {
            DRGInstallation::find()
                .as_ref()
                .map(DRGInstallation::main_pak)
        })
        .context("Could not find DRG pak file, please specify manually with the --fsd_pak flag")?;
    debug!(?game_pak_path);

    let mut state = State::init(dirs)?;

    let mut mods = Vec::new();
    state.mod_data.for_each_enabled_mod(&action.profile, |mc| {
        mods.push(mc.spec.clone());
    });

    resolve_unordered_and_integrate_with_provider_init(
        game_pak_path,
        &mut state,
        &mods,
        action.update,
        init_provider,
    )
    .await
}

async fn action_lint(dirs: Dirs, action: ActionLint) -> Result<()> {
    let game_pak_path = action
        .fsd_pak
        .or_else(|| {
            DRGInstallation::find()
                .as_ref()
                .map(DRGInstallation::main_pak)
        })
        .context("Could not find DRG pak file, please specify manually with the --fsd_pak flag")?;
    debug!(?game_pak_path);

    let mut state = State::init(dirs)?;

    let mut mods = Vec::new();
    state.mod_data.for_each_mod(&action.profile, |mc| {
        mods.push(mc.spec.clone());
    });

    let mod_paths = resolve_ordered_with_provider_init(&mut state, &mods, init_provider).await?;

    let report = tokio::task::spawn_blocking(move || {
        run_lints(
            &BTreeSet::from([
                LintId::ARCHIVE_WITH_ONLY_NON_PAK_FILES,
                LintId::ASSET_REGISTRY_BIN,
                LintId::CONFLICTING,
                LintId::EMPTY_ARCHIVE,
                LintId::OUTDATED_PAK_VERSION,
                LintId::SHADER_FILES,
                LintId::ARCHIVE_WITH_MULTIPLE_PAKS,
                LintId::NON_ASSET_FILES,
                LintId::SPLIT_ASSET_PAIRS,
            ]),
            mods.into_iter().zip(mod_paths).collect(),
            Some(game_pak_path),
        )
    })
    .await??;
    println!("{:#?}", report);
    Ok(())
}
