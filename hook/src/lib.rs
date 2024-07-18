mod hooks;
mod ue;
mod util;

use std::{io::BufReader, path::Path};

use anyhow::{Context, Result};
use fs_err as fs;
use hooks::{FnLoadGameFromMemory, FnSaveGameToMemory};
use mint_lib::mod_info::Meta;
use tracing::{info, warn};

proxy_dll::proxy_dll!([x3daudio1_7, d3d9], init);

fn init() {
    unsafe {
        patch().ok();
    }
}

static mut GLOBALS: Option<Globals> = None;
thread_local! {
    static LOG_GUARD: std::cell::RefCell<Option<tracing_appender::non_blocking::WorkerGuard>>  = None.into();
}

pub struct Globals {
    resolution: hook_resolvers::HookResolution,
    meta: Meta,
}

impl Globals {
    pub fn gmalloc(&self) -> &ue::FMalloc {
        unsafe {
            &**(self.resolution.core.as_ref().unwrap().gmalloc.0 as *const *const ue::FMalloc)
        }
    }
    pub fn fframe_step(&self) -> ue::FnFFrameStep {
        unsafe { std::mem::transmute(self.resolution.core.as_ref().unwrap().fframe_step.0) }
    }
    pub fn fframe_step_explicit_property(&self) -> ue::FnFFrameStepExplicitProperty {
        unsafe {
            std::mem::transmute(
                self.resolution
                    .core
                    .as_ref()
                    .unwrap()
                    .fframe_step_explicit_property
                    .0,
            )
        }
    }
    pub fn fname_to_string(&self) -> ue::FnFNameToString {
        unsafe { std::mem::transmute(self.resolution.core.as_ref().unwrap().fnametostring.0) }
    }
    pub fn fname_ctor_wchar(&self) -> ue::FnFNameCtorWchar {
        unsafe { std::mem::transmute(self.resolution.core.as_ref().unwrap().fname_ctor_wchar.0) }
    }
    pub fn uobject_base_utility_get_path_name(&self) -> ue::FnUObjectBaseUtilityGetPathName {
        unsafe {
            std::mem::transmute(
                self.resolution
                    .core
                    .as_ref()
                    .unwrap()
                    .uobject_base_utility_get_path_name
                    .0,
            )
        }
    }
    pub fn save_game_to_memory(&self) -> FnSaveGameToMemory {
        unsafe {
            std::mem::transmute(
                self.resolution
                    .save_game
                    .as_ref()
                    .unwrap()
                    .save_game_to_memory
                    .0,
            )
        }
    }
    pub fn load_game_from_memory(&self) -> FnLoadGameFromMemory {
        unsafe {
            std::mem::transmute(
                self.resolution
                    .save_game
                    .as_ref()
                    .unwrap()
                    .load_game_from_memory
                    .0,
            )
        }
    }
}

pub fn globals() -> &'static Globals {
    #[allow(static_mut_refs)]
    unsafe {
        GLOBALS.as_ref().unwrap()
    }
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

    GLOBALS = Some(Globals { resolution, meta });
    LOG_GUARD.with_borrow_mut(|g| *g = guard);

    hooks::initialize()?;

    info!("hook initialized");

    Ok(())
}
