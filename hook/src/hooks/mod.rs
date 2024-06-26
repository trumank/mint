#![allow(clippy::missing_transmute_annotations)]

mod server_list;

use std::{
    ffi::c_void,
    io::{Read, Seek},
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
};

use anyhow::{Context, Result};
use fs_err as fs;
use hook_resolvers::GasFixResolution;
use mint_lib::DRGInstallationType;
use sptr::OpaqueFnPtr;
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

    static FPakPlatformFileInitialize: unsafe extern "system" fn(*mut (), *mut (), *const ()) -> bool;
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

    FPakPlatformFileInitialize.initialize(
        std::mem::transmute(0x1431ce320usize),
        |this, inner, cmd_line| {
            tracing::info!("FPakPlatformFile::Initialize");
            let ret = FPakPlatformFileInitialize.call(this, inner, cmd_line);

            let mount: &&DelegateMount = std::mem::transmute(0x1462e2c20usize);
            let unmount: &&DelegateMount = std::mem::transmute(0x1462e2c30usize);

            let pak = &*mount.pak;
            tracing::info!("pak = {pak:#?}");

            for pak in pak.pak_files.as_slice().iter() {
                tracing::info!(
                    "mounted pak order={} path={}",
                    pak.read_order,
                    (*pak.pak_file).pak_filename
                );
            }

            ret
        },
    )?;
    FPakPlatformFileInitialize.enable()?;

    hook_virt()?;

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

#[derive(Debug)]
#[repr(C)]
struct FPakFile {
    vtable: *const (),
    idk: u64,
    idk2: *const (),
    pak_filename: ue::FString,
}

#[derive(Debug)]
#[repr(C)]
struct FPakListEntry {
    read_order: u32,
    pak_file: *const FPakFile,
}

#[derive(Debug)]
#[repr(C)]
struct FPakPlatformFile {
    vtable: *const (),
    lower_level: *const (),
    pak_files: ue::TArray<FPakListEntry>, // TODO ...
}

#[derive(Debug)]
#[repr(C)]
struct DelegateMount {
    idk1: u64,
    idk2: u64,
    idk3: u64,
    pak: *const FPakPlatformFile,
    call: unsafe extern "system" fn(*mut FPakPlatformFile, &ue::FString, u32) -> bool,
}
#[derive(Debug)]
#[repr(C)]
struct DelegateUnmount {
    idk1: u64,
    idk2: u64,
    idk3: u64,
    pak: *const FPakPlatformFile,
    call: unsafe extern "system" fn(*mut FPakPlatformFile, &ue::FString, u32) -> bool,
}

type FnVirt = unsafe extern "system" fn(a: *mut (), b: *mut (), c: *mut (), d: *mut ()) -> *mut ();

struct FPakVTable([Option<FnVirt>; 55]);

#[rustfmt::skip]
const VTABLE_NAMES: &[(*const (), &str)] = &[
    (hook_virt_n::< 0> as *const (), "__vecDelDtor"),
    (hook_virt_n::< 1> as *const (), "SetSandboxEnabled"),
    (hook_virt_n::< 2> as *const (), "IsSandboxEnabled"),
    (hook_virt_n::< 3> as *const (), "ShouldBeUsed"),
    (hook_virt_n::< 4> as *const (), "Initialize"),
    (hook_virt_n::< 5> as *const (), "InitializeAfterSetActive"),
    (hook_virt_n::< 6> as *const (), "MakeUniquePakFilesForTheseFiles"),
    (hook_virt_n::< 7> as *const (), "InitializeNewAsyncIO"),
    (hook_virt_n::< 8> as *const (), "AddLocalDirectories"),
    (hook_virt_n::< 9> as *const (), "BypassSecurity"),
    (hook_virt_n::<10> as *const (), "Tick"),
    (hook_virt_n::<11> as *const (), "GetLowerLevel"),
    (hook_virt_n::<12> as *const (), "SetLowerLevel"),
    (hook_virt_n::<13> as *const (), "GetName"),
    (hook_virt_n::<14> as *const (), "FileExists"),
    (hook_virt_n::<15> as *const (), "FileSize"),
    (hook_virt_n::<16> as *const (), "DeleteFile"),
    (hook_virt_n::<17> as *const (), "IsReadOnly"),
    (hook_virt_n::<18> as *const (), "MoveFile"),
    (hook_virt_n::<19> as *const (), "SetReadOnly"),
    (hook_virt_n::<20> as *const (), "GetTimeStamp"),
    (hook_virt_n::<21> as *const (), "SetTimeStamp"),
    (hook_virt_n::<22> as *const (), "GetAccessTimeStamp"),
    (hook_virt_n::<23> as *const (), "GetFilenameOnDisk"),
    (hook_open_read as *const (), "OpenRead"),
    (hook_open_read as *const (), "OpenReadNoBuffering"),
    (hook_virt_n::<26> as *const (), "OpenWrite"),
    (hook_virt_n::<27> as *const (), "DirectoryExists"),
    (hook_virt_n::<28> as *const (), "CreateDirectory"),
    (hook_virt_n::<29> as *const (), "DeleteDirectory"),
    (hook_virt_n::<30> as *const (), "GetStatData"),
    (hook_virt_n::<31> as *const (), "IterateDirectoryA"),
    (hook_virt_n::<32> as *const (), "IterateDirectoryB"),
    (hook_virt_n::<33> as *const (), "IterateDirectoryStatA"),
    (hook_virt_n::<34> as *const (), "IterateDirectoryStatB"),
    (hook_virt_n::<35> as *const (), "OpenAsyncRead"),
    (hook_virt_n::<36> as *const (), "SetAsyncMinimumPriority"),
    (hook_virt_n::<37> as *const (), "OpenMapped"),
    (hook_virt_n::<38> as *const (), "GetTimeStampPair"),
    (hook_virt_n::<39> as *const (), "GetTimeStampLocal"),
    (hook_virt_n::<40> as *const (), "IterateDirectoryRecursivelyA"),
    (hook_virt_n::<41> as *const (), "IterateDirectoryRecursivelyB"),
    (hook_virt_n::<42> as *const (), "IterateDirectoryStatRecursivelyA"),
    (hook_virt_n::<43> as *const (), "IterateDirectoryStatRecursivelyB"),
    (hook_virt_n::<44> as *const (), "FindFiles"),
    (hook_virt_n::<45> as *const (), "FindFilesRecursively"),
    (hook_virt_n::<46> as *const (), "DeleteDirectoryRecursively"),
    (hook_virt_n::<47> as *const (), "CreateDirectoryTree"),
    (hook_virt_n::<48> as *const (), "CopyFile"),
    (hook_virt_n::<49> as *const (), "CopyDirectoryTree"),
    (hook_virt_n::<50> as *const (), "ConvertToAbsolutePathForExternalAppForRead"),
    (hook_virt_n::<51> as *const (), "ConvertToAbsolutePathForExternalAppForWrite"),
    (hook_virt_n::<52> as *const (), "SendMessageToServer"),
    (hook_virt_n::<53> as *const (), "DoesCreatePublicFiles"),
    (hook_virt_n::<54> as *const (), "SetCreatePublicFiles"),
];

static mut VTABLE_ORIG: FPakVTable = FPakVTable([None; 55]);

#[repr(C)]
struct IFileHandle {
    vtable: *const IFileHandleVTable,
    file: std::fs::File,
}
impl IFileHandle {
    fn new(path: &str) -> Self {
        Self {
            vtable: &IFILE_HANDLE_VTABLE,
            file: std::fs::File::open(dbg!(std::path::Path::new("../../Content/Paks/mods_P/")
                .join(
                    std::path::Path::new(path)
                        .strip_prefix("../../../")
                        .unwrap(),
                ),))
            .expect("file open failed"),
        }
    }
    unsafe extern "system" fn __vec_del_dtor(this: *mut IFileHandle, _unknown: u32) {
        drop(Box::from_raw(this))
    }
    unsafe extern "system" fn tell(this: &mut IFileHandle) -> i64 {
        this.file.stream_position().expect("seek failed") as i64
    }
    unsafe extern "system" fn seek(this: &mut IFileHandle, new_position: i64) -> bool {
        this.file
            .seek(std::io::SeekFrom::Start(new_position as u64))
            .is_ok()
    }
    unsafe extern "system" fn seek_from_end(
        this: &mut IFileHandle,
        new_position_relative_to_end: i64,
    ) -> bool {
        this.file
            .seek(std::io::SeekFrom::End(new_position_relative_to_end))
            .is_ok()
    }
    unsafe extern "system" fn read(
        this: &mut IFileHandle,
        destination: *mut u8,
        bytes_to_read: i64,
    ) -> bool {
        this.file
            .read_exact(std::slice::from_raw_parts_mut(
                destination,
                bytes_to_read as usize,
            ))
            .is_ok()
    }
    unsafe extern "system" fn write(
        this: &mut IFileHandle,
        source: *const u8,
        bytes_to_write: i64,
    ) -> bool {
        unimplemented!("cannot write")
    }
    unsafe extern "system" fn flush(this: &mut IFileHandle, b_full_flush: bool) -> bool {
        unimplemented!("cannot flush")
    }
    unsafe extern "system" fn truncate(this: &mut IFileHandle, new_size: i64) -> bool {
        unimplemented!("cannot truncate")
    }
    unsafe extern "system" fn size(this: &mut IFileHandle) -> i64 {
        let Ok(cur) = this.file.seek(std::io::SeekFrom::Current(0)) else {
            return -1;
        };
        let Ok(size) = this.file.seek(std::io::SeekFrom::End(0)) else {
            return -1;
        };
        let Ok(_) = this.file.seek(std::io::SeekFrom::Start(cur)) else {
            return -1;
        };
        size as i64
    }
}

#[repr(C)]
struct IFileHandleVTable {
    __vec_del_dtor: unsafe extern "system" fn(*mut IFileHandle, u32),
    tell: unsafe extern "system" fn(&mut IFileHandle) -> i64,
    seek: unsafe extern "system" fn(&mut IFileHandle, i64) -> bool,
    seek_from_end: unsafe extern "system" fn(&mut IFileHandle, i64) -> bool,
    read: unsafe extern "system" fn(&mut IFileHandle, *mut u8, i64) -> bool,
    write: unsafe extern "system" fn(&mut IFileHandle, *const u8, i64) -> bool,
    flush: unsafe extern "system" fn(&mut IFileHandle, bool) -> bool,
    truncate: unsafe extern "system" fn(&mut IFileHandle, i64) -> bool,
    size: unsafe extern "system" fn(&mut IFileHandle) -> i64,
}
const IFILE_HANDLE_VTABLE: IFileHandleVTable = IFileHandleVTable {
    __vec_del_dtor: IFileHandle::__vec_del_dtor,
    tell: IFileHandle::tell,
    seek: IFileHandle::seek,
    seek_from_end: IFileHandle::seek_from_end,
    read: IFileHandle::read,
    write: IFileHandle::write,
    flush: IFileHandle::flush,
    truncate: IFileHandle::truncate,
    size: IFileHandle::size,
};

type FnHookOpenRead = unsafe extern "system" fn(
    this: *mut FPakPlatformFile,
    file_name: *const u16,
    b_allow_write: bool,
) -> *mut IFileHandle;
unsafe extern "system" fn hook_open_read(
    this: *mut FPakPlatformFile,
    file_name: *const u16,
    b_allow_write: bool,
) -> *mut IFileHandle {
    let name = widestring::U16CStr::from_ptr_str(file_name)
        .to_string()
        .unwrap();
    if name == "../../../FSD/Content/_AssemblyStorm/SandboxUtilities/SandboxUtilities.uexp" {
        return Box::into_raw(Box::new(IFileHandle::new(&name)));
    }
    //todo!("READ");
    let ret = std::mem::transmute::<_, FnHookOpenRead>(VTABLE_ORIG.0[24].unwrap())(
        this,
        file_name,
        b_allow_write,
    );
    ret
}

unsafe extern "system" fn hook_virt_n<const N: usize>(
    a: *mut (),
    b: *mut (),
    c: *mut (),
    d: *mut (),
) -> *mut () {
    //tracing::info!("FPakPlatformFile({N}={})", VTABLE_NAMES[N].1);
    (VTABLE_ORIG.0[N].unwrap())(a, b, c, d)
}

unsafe fn hook_virt() -> Result<()> {
    let addr = 0x1454a4580 as *mut FPakVTable;
    let size = std::mem::size_of::<FPakVTable>();

    let mut old = Default::default();
    VirtualProtect(
        addr as *const c_void,
        size,
        PAGE_EXECUTE_READWRITE,
        &mut old,
    )?;

    let vtable = &mut *addr;
    for (i, (virt, _name)) in VTABLE_NAMES.iter().enumerate() {
        (VTABLE_ORIG.0)[i] = vtable.0[i];
        vtable.0[i] = Some(std::mem::transmute(*virt));
    }

    VirtualProtect(addr as *const c_void, size, old, &mut old)?;

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
