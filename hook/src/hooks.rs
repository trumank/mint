#![allow(clippy::missing_transmute_annotations)]

use std::{
    cell::RefCell,
    ffi::c_void,
    mem::MaybeUninit,
    path::{Path, PathBuf},
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};

use anyhow::{Context, Result};
use element_ptr::element_ptr;
use fs_err as fs;
use hook_resolvers::GasFixResolution;
use mint_lib::DRGInstallationType;
use windows::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};

use crate::{globals, ue::*};

type ExecFn = unsafe extern "system" fn(*mut UObject, *mut kismet::FFrame, *mut c_void);

retour::static_detour! {
    static HookUFunctionBind: unsafe extern "system" fn(*mut UFunction);
    static GetServerName: unsafe extern "system" fn(*const c_void, *const c_void) -> *const FString;
    static SaveGameToSlot: unsafe extern "system" fn(*const USaveGame, *const FString, i32) -> bool;
    static LoadGameFromSlot: unsafe extern "system" fn(*const FString, i32) -> *const USaveGame;
    static DoesSaveGameExist: unsafe extern "system" fn(*const FString, i32) -> bool;
    static USessionHandlingFSDFillSessionSetting: unsafe extern "system" fn(*const c_void, *mut c_void, bool);
    static UObjectTemperatureComponentTimerCallback: unsafe extern "system" fn(*mut c_void);
    pub static UGameEngineTick: unsafe extern "system" fn(*mut c_void, f32, bool);
}

#[repr(C)]
pub struct USaveGame;

pub type FnSaveGameToMemory = unsafe extern "system" fn(*const USaveGame, *mut TArray<u8>) -> bool;
pub type FnLoadGameFromMemory = unsafe extern "system" fn(*const TArray<u8>) -> *const USaveGame;

struct HookPool([Hook; MAX_HOOKS]);

#[derive(Debug, Clone, Copy)]
struct Hook {
    native: ExecFn,
    function: Option<NonNull<UFunction>>,
}

const MAX_HOOKS: usize = 1000;
const fn gen_hooks() -> HookPool {
    let mut array: [MaybeUninit<Hook>; MAX_HOOKS] = [MaybeUninit::uninit(); MAX_HOOKS];

    seq_macro::seq!(N in 0..1000 { // TODO hardcoded
        array[N] = MaybeUninit::new(Hook {
            native: hook_exec_wrapper::<N> as ExecFn,
            function: None,
         });
    });

    HookPool(unsafe { std::mem::transmute(array) })
}

thread_local! {
    static HOOKS: RefCell<HookPool> = RefCell::new(gen_hooks())
}

unsafe extern "system" fn hook_exec_wrapper<const N: usize>(
    ctx: *mut UObject,
    frame: *mut kismet::FFrame,
    ret: *mut c_void,
) {
    hook_exec(N, ctx, frame, ret);
}

unsafe extern "system" fn hook_exec(
    n: usize,
    ctx: *mut UObject,
    stack: *mut kismet::FFrame,
    ret: *mut c_void,
) {
    let hook = HOOKS.with_borrow(|hooks| hooks.0[n]);
    hook.function.unwrap().ustruct().child_properties();

    let stack = stack.as_mut().unwrap();

    let ctx: Option<NonNull<UObject>> = stack.arg();
    let string: FString = stack.arg();
    let _print_to_screen: bool = stack.arg();
    let _print_to_log: bool = stack.arg();
    let _color: FLinearColor = stack.arg();
    let _duration: f32 = stack.arg();

    //if let Some(ctx) = ctx {
    //    let class = ctx.uobject_base().class();
    //    dbg!(class);
    //    if let Some(class) = class {
    //        class.ustruct().child_properties();
    //    }
    //}
    //

    println!("INSIDE HOOK {:?}", stack.current_native_function);

    crate::JS_CONTEXT.with_borrow(|js_ctx| {
        use deno_core::v8;

        let binding = js_ctx.runtime.as_ref().unwrap().borrow();
        let context = &binding.main_context;
        let isolate = binding.isolate;

        let mut scope = v8::HandleScope::with_context(
            unsafe { isolate.as_mut().unwrap_unchecked() },
            context.as_ref(),
        );
        let undefined: v8::Local<v8::Value> = v8::undefined(&mut scope).into();

        //let tc_scope = &mut v8::TryCatch::new(&mut scope);
        //let js_event_loop_tick_cb = context_state.js_event_loop_tick_cb.borrow();
        let binding = js_ctx.hooks.borrow();
        let js_event_loop_tick_cb = binding.values().next().unwrap().open(&mut scope);

        let js_obj = crate::deno_test::js_obj(&mut scope, ctx.unwrap());

        js_event_loop_tick_cb.call(&mut scope, undefined, &[js_obj.into()]);
    });

    //crate::JS_CONTEXT.with_borrow(|ctx| {
    //    if let Some(hook) = ctx.hooks.borrow().get(&path) {
    //        println!("INSIDE HOOK {:?}", hook);
    //    }
    //});

    //println!("{ctx:?} PrintString({string})");

    stack.code = stack.code.add(1);
}

pub unsafe fn initialize() -> Result<()> {
    let hooks = [
        (
            "/Game/_mint/BPL_MINT.BPL_MINT_C:Get Mod JSON",
            exec_get_mod_json as ExecFn,
        ),
        //(
        //    "/Script/Engine.KismetSystemLibrary:PrintString",
        //    exec_print_string as ExecFn,
        //),
    ]
    .into_iter()
    .collect::<std::collections::HashMap<_, ExecFn>>();

    HookUFunctionBind.initialize(
        std::mem::transmute(globals().resolution.core.as_ref().unwrap().ufunction_bind.0),
        move |function| {
            HookUFunctionBind.call(function);
            if let Some(function) = NonNull::new(function) {
                let path = function.uobject_base().get_path_name(None);
                if let Some(hook) = hooks.get(path.as_str()) {
                    element_ptr!(function => .function_flags)
                        .as_mut()
                        .insert(EFunctionFlags::FUNC_Native | EFunctionFlags::FUNC_Final);
                    *element_ptr!(function => .func).as_ptr() = *hook;
                }

                crate::JS_CONTEXT.with_borrow(|ctx| {
                    if let Some(hook) = ctx.hooks.borrow().get(&path) {
                        println!("HOOKING {} {:?}", path, function);

                        HOOKS.with_borrow_mut(|hooks| {
                            let free_hook = hooks
                                .0
                                .iter_mut()
                                .find(|h| h.function.is_none())
                                .expect("hooks exhausted");
                            free_hook.function = Some(function);

                            element_ptr!(function => .function_flags)
                                .as_mut()
                                .insert(EFunctionFlags::FUNC_Native | EFunctionFlags::FUNC_Final);
                            *element_ptr!(function => .func).as_ptr() = free_hook.native;
                        });
                    }
                });
            }
        },
    )?;
    HookUFunctionBind.enable()?;

    if let Ok(server_name) = &globals().resolution.server_name {
        GetServerName
            .initialize(
                std::mem::transmute(server_name.get_server_name.0),
                get_server_name_detour,
            )?
            .enable()?;
    }

    if let Ok(server_mods) = &globals().resolution.server_mods {
        let patch_addr = server_mods.semicolon_h_replace.0 as *mut u8;
        patch_mem(patch_addr.add(2), [0xC3])?;
        patch_mem(patch_addr.add(7), [0x0F, 0x1F, 0x44, 0x00, 0x00])?;
        patch_mem(patch_addr.add(102), [0xEB])?;

        let mods_fname = server_mods.mods_fname.0;
        let set_fstring = server_mods.set_fstring.0;
        USessionHandlingFSDFillSessionSetting
            .initialize(
                std::mem::transmute(server_mods.fill_session_setting.0),
                move |world, game_settings, full_server| {
                    USessionHandlingFSDFillSessionSetting.call(world, game_settings, full_server);

                    #[derive(serde::Serialize)]
                    struct Wrapper {
                        name: String,
                        version: String,
                        category: i32,
                    }

                    let s = serde_json::to_string(&vec![Wrapper {
                        name: globals().meta.to_server_list_string(),
                        version: "mint".into(),
                        category: 0,
                    }])
                    .unwrap();

                    let s = FString::from(s.as_str());

                    type Fn = unsafe extern "system" fn(
                        *const c_void,
                        *const c_void,
                        *const FString,
                        u32,
                    );

                    let f: Fn = std::mem::transmute(set_fstring);

                    f(game_settings, *(mods_fname as *const *const c_void), &s, 3);
                },
            )?
            .enable()?;
    }

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
            SAVES_DIR = Some(
                std::env::current_exe()
                    .ok()
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .context("could not determine save location")?
                    .join("Saved")
                    .join("SaveGames"),
            );

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
        padding: [u8; 0xc8],
        on_start_burning: u64,
        unknown: i64,
        temperature_change_scale: f32,
        burn_temperature: f32,
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

static mut SAVES_DIR: Option<PathBuf> = None;

fn get_path_for_slot(slot_name: &FString) -> Option<PathBuf> {
    let mut str_path = slot_name.to_string();
    str_path.push_str(".sav");

    let path = std::path::Path::new(&str_path);
    let mut normalized_path = unsafe { SAVES_DIR.as_ref() }?.clone();

    for component in path.components() {
        if let std::path::Component::Normal(c) = component {
            normalized_path.push(c)
        }
    }

    Some(normalized_path)
}

fn save_game_to_slot_detour(
    save_game_object: *const USaveGame,
    slot_name: *const FString,
    user_index: i32,
) -> bool {
    unsafe {
        let slot_name = &*slot_name;
        if slot_name.to_string() == "Player" {
            SaveGameToSlot.call(save_game_object, slot_name, user_index)
        } else {
            let mut data: TArray<u8> = Default::default();

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

fn load_game_from_slot_detour(slot_name: *const FString, user_index: i32) -> *const USaveGame {
    unsafe {
        let slot_name = &*slot_name;
        if slot_name.to_string() == "Player" {
            LoadGameFromSlot.call(slot_name, user_index)
        } else if let Some(data) = get_path_for_slot(slot_name).and_then(|path| fs::read(path).ok())
        {
            (globals().load_game_from_memory())(&TArray::from(data.as_slice()))
        } else {
            std::ptr::null()
        }
    }
}

fn does_save_game_exist_detour(slot_name: *const FString, user_index: i32) -> bool {
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

fn get_server_name_detour(a: *const c_void, b: *const c_void) -> *const FString {
    unsafe {
        let name = GetServerName.call(a, b).cast_mut().as_mut().unwrap();

        let mut new_name = widestring::U16String::new();
        new_name.push_str("[MODDED] ");
        new_name.push_slice(name.as_slice());

        name.clear();
        name.extend_from_slice(new_name.as_slice());
        name.push(0);

        name
    }
}

unsafe extern "system" fn exec_get_mod_json(
    _context: *mut UObject,
    stack: *mut kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let _ctx: Option<&UObject> = stack.arg();

    stack.most_recent_property_address = std::ptr::null();
    let ret: Option<FString> = stack.arg();
    let ret_address = (stack.most_recent_property_address as *mut FString)
        .as_mut()
        .unwrap();

    let json = serde_json::to_string(&globals().meta).unwrap();

    ret_address.clear();
    ret_address.extend_from_slice(&json.encode_utf16().chain([0]).collect::<Vec<_>>());

    std::mem::forget(ret);

    stack.code = stack.code.add(1);
}

unsafe extern "system" fn exec_print_string(
    _context: *mut UObject,
    stack: *mut kismet::FFrame,
    _result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let ctx: Option<NonNull<UObject>> = stack.arg();
    let string: FString = stack.arg();
    let _print_to_screen: bool = stack.arg();
    let _print_to_log: bool = stack.arg();
    let _color: FLinearColor = stack.arg();
    let _duration: f32 = stack.arg();

    //if let Some(ctx) = ctx {
    //    let class = ctx.uobject_base().class();
    //    dbg!(class);
    //    if let Some(class) = class {
    //        class.ustruct().child_properties();
    //    }
    //}

    println!("{ctx:?} PrintString({string})");

    stack.code = stack.code.add(1);
}
