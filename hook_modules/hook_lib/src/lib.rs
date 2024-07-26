use std::path::PathBuf;

use mint_lib::mod_info::Meta;

pub mod ue;
pub mod util;

static mut GLOBALS: Option<&'static Globals> = None;

pub fn init_globals(globals: &'static Globals) {
    unsafe { GLOBALS = Some(globals) }
}

pub fn globals() -> &'static Globals {
    unsafe { GLOBALS.unwrap() }
}

// TODO move these type definitions to a more logical place
#[repr(C)]
pub struct USaveGame;

pub type FnSaveGameToMemory =
    unsafe extern "system" fn(*const USaveGame, *mut ue::TArray<u8>) -> bool;
pub type FnLoadGameFromMemory =
    unsafe extern "system" fn(*const ue::TArray<u8>) -> *const USaveGame;

pub struct Globals {
    pub resolution: hook_resolvers::HookResolution,
    pub meta: Meta,
    pub bin_dir: Option<PathBuf>,
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
