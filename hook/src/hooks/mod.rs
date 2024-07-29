#![allow(clippy::missing_transmute_annotations)]

mod server_list;

use std::{
    collections::HashMap,
    ffi::c_void,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result};
use fs_err as fs;
use hook_resolvers::GasFixResolution;
use mint_lib::DRGInstallationType;
use windows::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};

use crate::{
    globals,
    ue::{self, kismet::FFrame, FLinearColor, UFunction, UObject},
    LOG_GUARD,
};

retour::static_detour! {
    static HookUFunctionBind: unsafe extern "system" fn(*mut ue::UFunction);
    static SaveGameToSlot: unsafe extern "system" fn(*const USaveGame, *const ue::FString, i32) -> bool;
    static LoadGameFromSlot: unsafe extern "system" fn(*const ue::FString, i32) -> *const USaveGame;
    static DoesSaveGameExist: unsafe extern "system" fn(*const ue::FString, i32) -> bool;
    static UObjectTemperatureComponentTimerCallback: unsafe extern "system" fn(*mut c_void);
    static WinMain: unsafe extern "system" fn(*mut (), *mut (), *mut (), i32, *const ()) -> i32;

}

#[repr(C)]
pub struct USaveGame;

pub type FnSaveGameToMemory =
    unsafe extern "system" fn(*const USaveGame, *mut ue::TArray<u8>) -> bool;
pub type FnLoadGameFromMemory =
    unsafe extern "system" fn(*const ue::TArray<u8>) -> *const USaveGame;

type ExecFn = unsafe extern "system" fn(*mut ue::UObject, *mut ue::kismet::FFrame, *mut c_void);

pub unsafe fn initialize() -> Result<()> {
    let hooks = [
        (
            "/Game/_mint/BPL_MINT.BPL_MINT_C:Get Mod JSON",
            exec_get_mod_json as ExecFn,
        ),
        (
            "/Script/Engine.KismetSystemLibrary:PrintString",
            exec_print_string as ExecFn,
        ),
    ]
    .iter()
    .chain(server_list::kismet_hooks().iter())
    .cloned()
    .collect::<std::collections::HashMap<_, ExecFn>>();

    WinMain.initialize(
        std::mem::transmute(globals().resolution.core.as_ref().unwrap().main.0),
        detour_main,
    )?;
    WinMain.enable()?;

    HookUFunctionBind.initialize(
        std::mem::transmute(globals().resolution.core.as_ref().unwrap().ufunction_bind.0),
        move |function| {
            HookUFunctionBind.call(function);
            if let Some(function) = function.as_mut() {
                let path = function
                    .ustruct
                    .ufield
                    .uobject
                    .uobject_base_utility
                    .uobject_base
                    .get_path_name(None);
                if let Some(hook) = hooks.get(path.as_str()) {
                    function
                        .function_flags
                        .insert(ue::EFunctionFlags::FUNC_Native);
                    function.func = *hook;
                }
            }
        },
    )?;
    HookUFunctionBind.enable()?;

    server_list::init_hooks()?;

    if !globals().meta.config.disable_fix_exploding_gas {
        if let Ok(gas_fix) = &globals().resolution.gas_fix {
            apply_gas_fix(gas_fix)?;
        }
    }

    let installation_type = DRGInstallationType::from_exe_path()?;

    match installation_type {
        DRGInstallationType::Steam => {
            if let Ok(address) = &globals().resolution.disable {
                patch_mem(
                    (address.0 as *mut u8).add(29),
                    [0xB8, 0x01, 0x00, 0x00, 0x00],
                )?;
            }
        }
        DRGInstallationType::Xbox => {
            let saves_dir = std::env::current_exe()
                .ok()
                .as_deref()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .and_then(Path::parent)
                .context("could not determine save location")?
                .join("Saved")
                .join("SaveGames");
            SAVES_DIR.get_or_init(|| saves_dir);

            if let Ok(save_game) = &globals().resolution.save_game {
                SaveGameToSlot
                    .initialize(
                        std::mem::transmute(save_game.save_game_to_slot.0),
                        save_game_to_slot_detour,
                    )?
                    .enable()?;
                LoadGameFromSlot
                    .initialize(
                        std::mem::transmute(save_game.load_game_from_slot.0),
                        load_game_from_slot_detour,
                    )?
                    .enable()?;

                DoesSaveGameExist
                    .initialize(
                        std::mem::transmute(save_game.does_save_game_exist.0),
                        does_save_game_exist_detour,
                    )?
                    .enable()?;
            }
        }
    }
    Ok(())
}

unsafe fn apply_gas_fix(gas_fix: &Arc<GasFixResolution>) -> Result<()> {
    #[repr(C)]
    struct UObjectTemperatureComponent {
        padding: [u8; 0xd8],
        on_start_burning: [u64; 2],
        on_frozen_server: [u64; 2],
        temperature_change_scale: f32,
        burn_temperature: f32,
        freeze_temperature: f32,
        douse_fire_temperature: f32,
        cooling_rate: f32,
        is_heatsource_when_on_fire: bool,
        on_fire_heat_range: f32,
        timer_handle: u64,
        is_object_on_fire: bool,
        current_temperature: f32,
    }

    let fn_process_multicast_delegate: unsafe extern "system" fn(*mut c_void, *mut c_void) =
        std::mem::transmute(gas_fix.process_multicast_delegate.0);

    UObjectTemperatureComponentTimerCallback.initialize(
        std::mem::transmute(gas_fix.timer_callback.0),
        move |this| {
            let obj = &*(this as *const UObjectTemperatureComponent);
            let on_fire = obj.is_object_on_fire;
            UObjectTemperatureComponentTimerCallback.call(this);
            if !on_fire && obj.is_object_on_fire {
                fn_process_multicast_delegate(
                    std::ptr::addr_of!(obj.on_start_burning) as *mut c_void,
                    std::ptr::null_mut(),
                );
            }
        },
    )?;
    UObjectTemperatureComponentTimerCallback.enable()?;
    Ok(())
}

unsafe fn patch_mem(address: *mut u8, patch: impl AsRef<[u8]>) -> Result<()> {
    let patch = patch.as_ref();
    let patch_mem = std::slice::from_raw_parts_mut(address, patch.len());

    let mut old = Default::default();
    VirtualProtect(
        patch_mem.as_ptr() as *const c_void,
        patch_mem.len(),
        PAGE_EXECUTE_READWRITE,
        &mut old,
    )?;

    patch_mem.copy_from_slice(patch);

    VirtualProtect(
        patch_mem.as_ptr() as *const c_void,
        patch_mem.len(),
        old,
        &mut old,
    )?;

    Ok(())
}

static SAVES_DIR: OnceLock<PathBuf> = OnceLock::new();

fn get_path_for_slot(slot_name: &ue::FString) -> Option<PathBuf> {
    let mut str_path = slot_name.to_string();
    str_path.push_str(".sav");

    let path = std::path::Path::new(&str_path);
    let mut normalized_path = SAVES_DIR.get().unwrap().clone();

    for component in path.components() {
        if let std::path::Component::Normal(c) = component {
            normalized_path.push(c)
        }
    }

    Some(normalized_path)
}

fn save_game_to_slot_detour(
    save_game_object: *const USaveGame,
    slot_name: *const ue::FString,
    user_index: i32,
) -> bool {
    unsafe {
        let slot_name = &*slot_name;
        if slot_name.to_string() == "Player" {
            SaveGameToSlot.call(save_game_object, slot_name, user_index)
        } else {
            let mut data: ue::TArray<u8> = Default::default();

            if !(globals().save_game_to_memory())(save_game_object, &mut data) {
                return false;
            }

            let Some(path) = get_path_for_slot(slot_name) else {
                return false;
            };

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).ok();
            }

            let res = fs::write(path, data.as_slice()).is_ok();
            res
        }
    }
}

fn load_game_from_slot_detour(slot_name: *const ue::FString, user_index: i32) -> *const USaveGame {
    unsafe {
        let slot_name = &*slot_name;
        if slot_name.to_string() == "Player" {
            LoadGameFromSlot.call(slot_name, user_index)
        } else if let Some(data) = get_path_for_slot(slot_name).and_then(|path| fs::read(path).ok())
        {
            (globals().load_game_from_memory())(&ue::TArray::from(data.as_slice()))
        } else {
            std::ptr::null()
        }
    }
}

fn does_save_game_exist_detour(slot_name: *const ue::FString, user_index: i32) -> bool {
    unsafe {
        let slot_name = &*slot_name;
        if slot_name.to_string() == "Player" {
            DoesSaveGameExist.call(slot_name, user_index)
        } else if let Some(path) = get_path_for_slot(slot_name) {
            path.exists()
        } else {
            false
        }
    }
}

struct NativesArray([Option<ExecFn>; 0x100]);

static mut GNATIVES_OLD: NativesArray = NativesArray([None; 0x100]);
static mut NAME_CACHE: Option<HashMap<usize, String>> = None;

unsafe fn debug(expr: usize, ctx: *mut UObject, frame: *mut FFrame, ret: *mut c_void) {
    if NAME_CACHE.is_none() {
        NAME_CACHE = Some(Default::default());
    }

    let (index, path) = {
        let stack = &*frame;
        let func = &*(stack.node as *const UFunction);

        let index = (stack.code as isize).wrapping_sub(func.ustruct.script.as_ptr() as isize);

        let path = NAME_CACHE
            .as_mut()
            .unwrap_unchecked()
            .entry(stack.node as usize)
            .or_insert_with(|| {
                func.ustruct
                    .ufield
                    .uobject
                    .uobject_base_utility
                    .uobject_base
                    .get_path_name(None)
            });
        (index, path)
    };

    ((GNATIVES_OLD.0)[expr].unwrap())(ctx, frame, ret);
}

unsafe extern "system" fn hook_exec<const N: usize>(
    ctx: *mut UObject,
    frame: *mut FFrame,
    ret: *mut c_void,
) {
    debug(N, ctx, frame, ret);
}

unsafe fn hook_gnatives(gnatives: &mut NativesArray) {
    seq_macro::seq!(N in 0..256 {
        (GNATIVES_OLD.0)[N] = gnatives.0[N];
        gnatives.0[N] = Some(hook_exec::<N>);
    });
}

fn detour_main(
    h_instance: *mut (),
    h_prev_instance: *mut (),
    lp_cmd_line: *mut (),
    n_cmd_show: i32,
    cmd_line: *const (),
) -> i32 {
    unsafe {
        if let Ok(debug) = &globals().resolution.debug {
            tracing::info!("hooking GNatives");
            hook_gnatives((debug.gnatives.0 as *mut NativesArray).as_mut().unwrap());
        }

        let ret = WinMain.call(
            h_instance,
            h_prev_instance,
            lp_cmd_line,
            n_cmd_show,
            cmd_line,
        );

        // about to exit, drop log guard
        drop(LOG_GUARD.with_borrow_mut(|g| g.take()).unwrap());

        ret
    }
}

unsafe extern "system" fn exec_get_mod_json(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let _ctx: Option<&ue::UObject> = stack.arg();

    stack.most_recent_property_address = std::ptr::null();
    let ret: Option<ue::FString> = stack.arg();
    let ret_address = (stack.most_recent_property_address as *mut ue::FString)
        .as_mut()
        .unwrap();

    let json = serde_json::to_string(&globals().meta).unwrap();

    ret_address.clear();
    ret_address.extend_from_slice(&json.encode_utf16().chain([0]).collect::<Vec<_>>());

    std::mem::forget(ret);

    stack.code = stack.code.add(1);
}

unsafe extern "system" fn exec_print_string(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let _ctx: Option<NonNull<UObject>> = stack.arg();
    let string: ue::FString = stack.arg();
    let _print_to_screen: bool = stack.arg();
    let _print_to_log: bool = stack.arg();
    let _color: FLinearColor = stack.arg();
    let _duration: f32 = stack.arg();

    println!("PrintString({string})");

    stack.code = stack.code.add(1);
}
