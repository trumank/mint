#![allow(clippy::missing_transmute_annotations)]

mod server_list;

use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
};

use anyhow::{Context, Result};
use fs_err as fs;
use hook_resolvers::GasFixResolution;
use mint_lib::DRGInstallationType;
use windows::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};

use crate::{
    globals,
    ue::{self, FLinearColor, UObject},
    LOG_GUARD,
};

retour::static_detour! {
    static HookUFunctionBind: unsafe extern "system" fn(*mut ue::UFunction);
    static SaveGameToSlot: unsafe extern "system" fn(*const USaveGame, *const ue::FString, i32) -> bool;
    static LoadGameFromSlot: unsafe extern "system" fn(*const ue::FString, i32) -> *const USaveGame;
    static DoesSaveGameExist: unsafe extern "system" fn(*const ue::FString, i32) -> bool;
    static UObjectTemperatureComponentTimerCallback: unsafe extern "system" fn(*mut c_void);
    static WinMain: unsafe extern "system" fn(*mut (), *mut (), *mut (), i32, *const ()) -> i32;

    static Serialize: unsafe extern "system" fn(*mut (), *mut u8, i64);
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
                        .insert(ue::EFunctionFlags::FUNC_Native | ue::EFunctionFlags::FUNC_Final);
                    function.func = *hook;
                }
            }
        },
    )?;
    HookUFunctionBind.enable()?;

    #[repr(C)]
    struct FMemoryReader {
        padding1: [u8; 0x98],
        offset: u64,
    }

    Serialize
        .initialize(
            std::mem::transmute(0x14116d8b0usize),
            move |this, data, count| {
                let a: &FMemoryReader = &*(this as *const FMemoryReader);
                let offset = a.offset;
                //tracing::info!("Serialize({this:?}, {offset}, {count}, {data:?})");
                //tracing::info!("{:#?}", std::backtrace::Backtrace::force_capture());
                //let bt = backtrace::Backtrace::new();
                let mut stack = vec![];
                let mut flag = false;
                backtrace::trace(|frame| {
                    let ip = frame.ip() as u64;
                    if ip >= 0x140000000 && ip <= 0x14f000000 {
                        stack.push(ip);
                        flag = true;
                        true
                    } else {
                        !flag
                    }
                });
                stack.reverse();
                //tracing::info!("stack: {stack:x?}");
                Serialize.call(this, data, count);
                OPS.push(Op {
                    data: if count == 0 {
                        vec![]
                    } else {
                        std::slice::from_raw_parts(data, count as usize).to_vec()
                    },
                    offset: offset as usize,
                    count: count as usize,
                    stack,
                });
            },
        )
        .unwrap();

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
            if let Ok(save_game) = &globals().resolution.save_game {
                LoadGameFromSlot
                    .initialize(
                        std::mem::transmute(save_game.load_game_from_slot.0),
                        load_game_from_slot_detour,
                    )?
                    .enable()?;
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

static mut SAVES_DIR: Option<PathBuf> = None;

fn get_path_for_slot(slot_name: &ue::FString) -> Option<PathBuf> {
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

mod hw_breakpoints {
    use windows::Win32::Foundation::EXCEPTION_SINGLE_STEP;
    use windows::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, GetThreadContext, RemoveVectoredExceptionHandler,
        SetThreadContext, CONTEXT, CONTEXT_CONTROL_AMD64, CONTEXT_DEBUG_REGISTERS_AMD64,
        EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_POINTERS,
    };
    use windows::Win32::System::Threading::GetCurrentThread;

    const HW_BREAKPOINT_LEN_1: u64 = 0b00;
    const HW_BREAKPOINT_TYPE_WRITE: u64 = 0b01;

    pub unsafe fn set_hardware_breakpoint(
        address: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("Hooking addr {:X}", address);
        let mut context = CONTEXT {
            ContextFlags: CONTEXT_DEBUG_REGISTERS_AMD64 | CONTEXT_CONTROL_AMD64,
            ..Default::default()
        };

        GetThreadContext(GetCurrentThread(), &mut context)?;

        context.Dr0 = address as u64;
        context.Dr7 |= (1 << 0) | (HW_BREAKPOINT_TYPE_WRITE << 16) | (HW_BREAKPOINT_LEN_1 << 18);

        SetThreadContext(GetCurrentThread(), &context)?;

        Ok(())
    }

    pub unsafe fn remove_hardware_breakpoint() -> Result<(), Box<dyn std::error::Error>> {
        let mut context = CONTEXT {
            ContextFlags: CONTEXT_DEBUG_REGISTERS_AMD64 | CONTEXT_CONTROL_AMD64,
            ..Default::default()
        };

        GetThreadContext(GetCurrentThread(), &mut context)?;

        context.Dr0 = 0;
        context.Dr7 &= !(1 << 0);

        SetThreadContext(GetCurrentThread(), &context)?;

        Ok(())
    }

    pub unsafe extern "system" fn exception_handler(
        exception_info: *mut EXCEPTION_POINTERS,
    ) -> i32 {
        let exception_info = &*exception_info;
        if (*exception_info.ExceptionRecord).ExceptionCode == EXCEPTION_SINGLE_STEP {
            let context = &*exception_info.ContextRecord;
            let address = context.Dr0 as *const u32;
            let rip = context.Rip;
            let value = *address;
            (CB.as_mut().unwrap())(rip);
            tracing::info!("Hit {:x} {:p}, value: {}", rip, address, value);

            return EXCEPTION_CONTINUE_EXECUTION;
        }
        0
    }

    //static mut CB: *mut () = std::ptr::null_mut();
    static mut CB: Option<Box<dyn FnMut(u64) -> ()>> = None;
    pub unsafe fn hook<C, F, R>(
        address: usize,
        cb: C,
        exec: F,
    ) -> Result<R, Box<dyn std::error::Error>>
    where
        C: FnMut(u64) -> (),
        F: FnOnce() -> R,
    {
        let r: Box<dyn FnMut(u64) -> ()> = Box::new(cb);
        CB = Some(std::mem::transmute_copy(&r));
        std::mem::forget(r);
        let handle = AddVectoredExceptionHandler(1, Some(exception_handler));
        set_hardware_breakpoint(address)?;
        tracing::info!("calling");
        let ret = exec();
        tracing::info!("done calling");
        remove_hardware_breakpoint()?;
        RemoveVectoredExceptionHandler(handle);
        CB.take();
        Ok(ret)
    }

    fn main() {
        let mut counter = 10;

        let memory_address_to_monitor = std::ptr::addr_of!(counter) as usize;

        unsafe {
            AddVectoredExceptionHandler(1, Some(exception_handler));

            match set_hardware_breakpoint(memory_address_to_monitor) {
                Ok(_) => println!("Hardware breakpoint set successfully!"),
                Err(e) => eprintln!("Failed to set hardware breakpoint: {:?}", e),
            }
        }

        println!("Entered main loop");
        for _ in 0..10 {
            counter += 1;
            println!("inc");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

struct Op {
    data: Vec<u8>,
    offset: usize,
    count: usize,
    stack: Vec<u64>,
}

#[derive(Debug, serde::Serialize)]
#[repr(transparent)]
pub struct TreeSpan(pub ReadSpan<TreeSpan>);

#[derive(Debug, serde::Serialize)]
pub struct ReadSpan<S> {
    pub name: std::borrow::Cow<'static, str>,
    pub actions: Vec<Action<S>>,
}
#[derive(Debug, serde::Serialize)]
pub enum Action<S> {
    Read(usize),
    Seek(usize),
    Span(S),
}
#[derive(Debug, serde::Serialize)]
pub struct Trace {
    #[serde(with = "base64")]
    pub data: Vec<u8>,
    pub root: TreeSpan,
}
mod base64 {
    use serde::{Deserialize, Serialize};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        let base64 = base64::encode(v);
        String::serialize(&base64, s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let base64 = String::deserialize(d)?;
        base64::decode(base64.as_bytes()).map_err(|e| serde::de::Error::custom(e))
    }
}

fn save(name: &str, ops: Vec<Op>) {
    tracing::info!("ops={}", ops.len());
    let mut data = vec![];
    for op in &ops {
        assert_eq!(op.offset, data.len());
        data.extend_from_slice(&op.data);
    }

    #[derive(Debug)]
    enum TreeNode {
        Frame(Frame),
        Read { count: usize },
    }
    impl Into<Action<TreeSpan>> for TreeNode {
        fn into(self) -> Action<TreeSpan> {
            match self {
                TreeNode::Frame(frame) => Action::Span(TreeSpan(ReadSpan::<TreeSpan> {
                    name: format!("0x{:X}", frame.id).into(),
                    actions: frame.children.into_iter().map(|c| c.into()).collect(),
                })),
                TreeNode::Read { count } => Action::Read(count),
            }
        }
    }

    #[derive(Debug)]
    struct Frame {
        id: u64,
        children: Vec<TreeNode>,
    }

    impl Frame {
        fn new(id: u64) -> Self {
            Frame {
                id,
                children: Vec::new(),
            }
        }
        fn insert(&mut self, path: &[u64], count: usize) {
            if path.is_empty() {
                self.children.push(TreeNode::Read { count });
                return;
            }

            let child_id = path[0];
            let rest = &path[1..];

            match self.children.last_mut() {
                Some(TreeNode::Frame(frame)) if frame.id == child_id => {
                    frame.insert(rest, count);
                    return;
                }
                _ => {}
            }

            let mut new_child = Frame::new(child_id);
            new_child.insert(rest, count);
            self.children.push(TreeNode::Frame(new_child));
        }
    }

    //if ops.is_empty() {
    //    return None;
    //}

    let mut root = Frame::new(ops[0].stack[0]);
    for op in ops {
        root.insert(&op.stack, op.count);
    }
    let trace = Trace {
        data,
        root: TreeSpan(ReadSpan {
            name: "asdf".into(),
            actions: vec![TreeNode::Frame(root).into()],
        }),
    };

    let s = serde_json::to_vec(&trace).unwrap();
    std::fs::write(format!("{name}.json"), s).unwrap();

    //println!("{trace:#X?}");
}

static mut OPS: Vec<Op> = vec![];
fn load_game_from_slot_detour(slot_name: *const ue::FString, user_index: i32) -> *const USaveGame {
    unsafe {
        //let slot_name = &*slot_name;
        //let slot = slot_name.to_string();
        //tracing::info!("Loading game {slot}");
        //tracing::info_span!("LoadGameFromSlot", slot).in_scope(|| {
        //    let mut asdf = HashSet::new();
        //    hw_breakpoints::hook(
        //        std::ptr::addr_of!(asdf) as usize - 0x1c8 - 0x70,
        //        |addr| {
        //            asdf.insert(addr);
        //        },
        //        || {
        //            tracing::info!("calling");
        //            let ret = LoadGameFromSlot.call(slot_name, user_index);
        //            tracing::info!("done calling");
        //            ret
        //        },
        //    )
        //    .unwrap()
        //})

        let slot_name = &*slot_name;
        let slot = slot_name.to_string();
        tracing::info!("Loading game {slot}");
        tracing::info_span!("LoadGameFromSlot", slot).in_scope(|| {
            //let mut asdf = HashSet::new();

            OPS.clear();
            Serialize.enable().unwrap();
            let ret = LoadGameFromSlot.call(slot_name, user_index);
            Serialize.disable().unwrap();
            save(&slot, std::mem::take(&mut OPS));
            ret
        })
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

fn detour_main(
    h_instance: *mut (),
    h_prev_instance: *mut (),
    lp_cmd_line: *mut (),
    n_cmd_show: i32,
    cmd_line: *const (),
) -> i32 {
    let ret = unsafe {
        WinMain.call(
            h_instance,
            h_prev_instance,
            lp_cmd_line,
            n_cmd_show,
            cmd_line,
        )
    };

    // about to exit, drop log guard
    drop(unsafe { LOG_GUARD.take() });

    ret
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
