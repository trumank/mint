mod hooks;

use std::{
    io::BufReader,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use fs_err as fs;
use hook_lib::{init_globals, Globals};
use mint_lib::mod_info::Meta;
use tracing::{info, warn};

proxy_dll::proxy_dll!([x3daudio1_7, d3d9], init);

fn init() {
    unsafe {
        patch().ok();
    }
}

thread_local! {
    static LOG_GUARD: std::cell::RefCell<Option<tracing_appender::non_blocking::WorkerGuard>>  = None.into();
}

unsafe fn patch() -> Result<()> {
    let exe_path = std::env::current_exe().ok();
    let bin_dir = exe_path.as_deref().and_then(Path::parent);

    let guard = bin_dir
        .and_then(|bin_dir| mint_lib::setup_logging(bin_dir.join("mint_hook.log"), "hook").ok());
    if guard.is_none() {
        warn!("failed to set up logging");
    }

    let pak_path = bin_dir
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|p| p.join("Content/Paks/mods_P.pak"))
        .context("could not determine pak path")?;

    let mut pak_reader = BufReader::new(fs::File::open(pak_path)?);
    let pak = repak::PakBuilder::new().reader(&mut pak_reader)?;

    let meta_buf = pak.get("meta", &mut pak_reader)?;
    let meta: Meta = postcard::from_bytes(&meta_buf)?;

    let image = patternsleuth::process::internal::read_image()?;
    let resolution = image.resolve(hook_resolvers::HookResolution::resolver())?;
    info!("PS scan: {:#x?}", resolution);

    static mut GLOBALS: Option<Globals> = None;
    GLOBALS = Some(Globals {
        resolution,
        meta,
        bin_dir: bin_dir.map(|d| d.to_path_buf()),
    });
    init_globals(GLOBALS.as_ref().unwrap());
    LOG_GUARD.with_borrow_mut(|g| *g = guard);

    hooks::initialize()?;

    info!("hook initialized");

    Ok(())
}
