pub mod deno_test;
mod hooks;
mod ue;

use std::{cell::RefCell, collections::HashMap, io::BufReader, path::Path, rc::Rc};

use anyhow::{Context, Result};
use deno_core::futures::FutureExt;
use fs_err as fs;
use hooks::{FnLoadGameFromMemory, FnSaveGameToMemory};
use mint_lib::mod_info::Meta;
use tracing::{info, warn};
use ue::FnStaticFindObjectFast;
use windows::Win32::{
    Foundation::HMODULE,
    System::{
        SystemServices::*,
        Threading::{GetCurrentThread, QueueUserAPC},
    },
};

// x3daudio1_7.dll
#[no_mangle]
#[allow(non_snake_case, unused_variables)]
extern "system" fn X3DAudioCalculate() {}
#[no_mangle]
#[allow(non_snake_case, unused_variables)]
extern "system" fn X3DAudioInitialize() {}

// d3d9.dll
#[no_mangle]
#[allow(non_snake_case, unused_variables)]
extern "system" fn D3DPERF_EndEvent() {}
#[no_mangle]
#[allow(non_snake_case, unused_variables)]
extern "system" fn D3DPERF_BeginEvent() {}

#[no_mangle]
#[allow(non_snake_case, unused_variables)]
extern "system" fn DllMain(dll_module: HMODULE, call_reason: u32, _: *mut ()) -> bool {
    unsafe {
        match call_reason {
            DLL_PROCESS_ATTACH => {
                QueueUserAPC(Some(init), GetCurrentThread(), 0);
            }
            DLL_PROCESS_DETACH => (),
            _ => (),
        }

        true
    }
}

unsafe extern "system" fn init(_: usize) {
    patch().ok();
}

static mut GLOBALS: Option<Globals> = None;

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
    pub fn fname_ctor(&self) -> ue::FnFNameCtor {
        unsafe { std::mem::transmute(self.resolution.core.as_ref().unwrap().fname_ctor.0) }
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
    pub fn static_find_object_fast(&self) -> FnStaticFindObjectFast {
        unsafe {
            std::mem::transmute(
                globals()
                    .resolution
                    .core
                    .as_ref()
                    .unwrap()
                    .static_find_object_fast
                    .0,
            )
        }
    }
}

pub fn globals() -> &'static Globals {
    unsafe { GLOBALS.as_ref().unwrap() }
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

    hooks::initialize()?;

    info!("hook initialized");

    init_js();

    Ok(())
}

use deno_core::v8;

#[derive(Default)]
struct JsContextHandle {
    task: Option<Rc<RefCell<tokio::task::JoinHandle<()>>>>,
    runtime: Option<Rc<RefCell<deno_test::JsUeRuntime>>>,
    hooks: RefCell<HashMap<String, v8::Global<v8::Function>>>,
}

thread_local! {
    static JS_CONTEXT: std::cell::RefCell<JsContextHandle> = Default::default();
}

unsafe fn init_js() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();

    let mut js_runtime = deno_test::JsUeRuntime::new();
    rt.block_on(js_runtime.init()).unwrap();
    let js_runtime = Rc::new(RefCell::new(js_runtime));

    let task = rt.spawn(
        unsafe {
            let js_runtime = js_runtime.clone();
            deno_core::unsync::MaskFutureAsSend::new(async move {
                println!("now running on a worker thread");
                js_runtime.borrow().run_loop().await.unwrap();
            })
        }
        .map(|_| ()),
    );

    JS_CONTEXT.with_borrow_mut(move |ctx| {
        ctx.task = Some(Rc::new(RefCell::new(task)));
        ctx.runtime = Some(js_runtime);
    });

    hooks::UGameEngineTick
        .initialize(
            std::mem::transmute(
                globals()
                    .resolution
                    .core
                    .as_ref()
                    .unwrap()
                    .game_engine_tick
                    .0,
            ),
            move |game_engine, delta_seconds, idle_mode| {
                hooks::UGameEngineTick.call(game_engine, delta_seconds, idle_mode);
                JS_CONTEXT.with_borrow(|ctx| {
                    rt.block_on(async {
                        let binding = ctx.task.as_ref().unwrap();
                        tokio::select! {
                            _ = &mut *binding.borrow_mut() => Some(Ok::<(), tokio::task::JoinError>(())),
                            _ = tokio::task::yield_now() => None,
                        }
                    });
                });
            },
        )
        .unwrap();
    hooks::UGameEngineTick.enable().unwrap();
}
