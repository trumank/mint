use std::{
    ffi::{c_void, OsString},
    io::BufReader,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use mint_lib::{mod_info::Meta, DRGInstallationType};
use retour::static_detour;
use windows::Win32::{
    Foundation::HMODULE,
    System::{
        Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE},
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

unsafe fn patch() -> Result<()> {
    let pak_path = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|p| p.join("Content/Paks/mods_P.pak"))
        .context("could not determine pak path")?;

    let mut pak_reader = BufReader::new(std::fs::File::open(pak_path)?);
    let pak = repak::PakBuilder::new().reader(&mut pak_reader)?;

    let meta_buf = pak.get("meta", &mut pak_reader)?;
    let meta: Meta = postcard::from_bytes(&meta_buf)?;

    let installation_type = DRGInstallationType::from_exe_path()?;

    let image = patternsleuth::process::internal::read_image()?;
    let resolution = image.resolve(hook_resolvers::HookResolution::resolver())?;
    println!("{:#x?}", resolution);

    if let Ok(fmemory_free) = resolution.fmemory_free {
        Free = Some(std::mem::transmute(fmemory_free.0));
    }

    if let Ok(server_name) = resolution.server_name {
        Resize16 = Some(std::mem::transmute(server_name.resize16.0));

        GetServerName
            .initialize(
                std::mem::transmute(server_name.get_server_name.0),
                get_server_name_detour,
            )?
            .enable()?;
    }

    if let Ok(server_mods) = resolution.server_mods {
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
                        name: meta.to_server_list_string(),
                        version: "mint".into(),
                        category: 0,
                    }])
                    .unwrap();

                    let buf = s.encode_utf16().chain([0]).collect::<Vec<_>>();
                    let s = FString::from_slice(&buf);

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

    if let Ok(gas_fix) = resolution.gas_fix {
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
    }

    match installation_type {
        DRGInstallationType::Steam => {
            if let Ok(address) = resolution.disable {
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

            if let Ok(save_game) = resolution.save_game {
                SaveGameToMemory = Some(std::mem::transmute(save_game.save_game_to_memory.0));
                LoadGameFromMemory = Some(std::mem::transmute(save_game.load_game_from_memory.0));

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

type FString = TArray<u16>;

#[derive(Debug)]
#[repr(C)]
struct TArray<T> {
    data: *const T,
    num: i32,
    max: i32,
}

#[repr(C)]
struct USaveGame;

impl<T> TArray<T> {
    fn as_slice(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.data, self.num as usize) }
    }
    fn as_slice_mut(&mut self) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.data as *mut _, self.num as usize) }
    }
    fn from_slice(slice: &[T]) -> TArray<T> {
        TArray {
            data: slice.as_ptr(),
            num: slice.len() as i32,
            max: slice.len() as i32,
        }
    }
}

impl FString {
    fn to_os_string(&self) -> OsString {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::ffi::OsStringExt;
            let slice = self.as_slice();
            let len = slice
                .iter()
                .enumerate()
                .find_map(|(i, &b)| (b == 0).then_some(i))
                .unwrap_or(slice.len());
            std::ffi::OsString::from_wide(&slice[0..len])
        }
        #[cfg(not(target_os = "windows"))]
        unimplemented!()
    }
}

type FnFree = unsafe extern "system" fn(*const c_void);
type FnResize16 = unsafe extern "system" fn(*const c_void, new_max: i32);
type FnSaveGameToMemory = unsafe extern "system" fn(*const USaveGame, *mut TArray<u8>) -> bool;
type FnLoadGameFromMemory = unsafe extern "system" fn(*const TArray<u8>) -> *const USaveGame;

static_detour! {
    static GetServerName: unsafe extern "system" fn(*const c_void, *const c_void) -> *const FString;
    static SaveGameToSlot: unsafe extern "system" fn(*const USaveGame, *const FString, i32) -> bool;
    static LoadGameFromSlot: unsafe extern "system" fn(*const FString, i32) -> *const USaveGame;
    static DoesSaveGameExist: unsafe extern "system" fn(*const FString, i32) -> bool;
    static USessionHandlingFSDFillSessionSetting: unsafe extern "system" fn(*const c_void, *mut c_void, bool);
    static UObjectTemperatureComponentTimerCallback: unsafe extern "system" fn(*mut c_void);
}

#[allow(non_upper_case_globals)]
static mut Free: Option<FnFree> = None;
#[allow(non_upper_case_globals)]
static mut Resize16: Option<FnResize16> = None;
#[allow(non_upper_case_globals)]
static mut SaveGameToMemory: Option<FnSaveGameToMemory> = None;
#[allow(non_upper_case_globals)]
static mut LoadGameFromMemory: Option<FnLoadGameFromMemory> = None;

static mut SAVES_DIR: Option<PathBuf> = None;

fn get_path_for_slot(slot_name: &FString) -> Option<PathBuf> {
    let mut str_path = slot_name.to_os_string();
    str_path.push(".sav");

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
        if slot_name.to_os_string().to_string_lossy() == "Player" {
            SaveGameToSlot.call(save_game_object, slot_name, user_index)
        } else {
            let mut data = TArray::<u8> {
                data: std::ptr::null(),
                num: 0,
                max: 0,
            };

            if !SaveGameToMemory.unwrap()(save_game_object, &mut data) {
                return false;
            }

            let Some(path) = get_path_for_slot(slot_name) else {
                return false;
            };

            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            let res = std::fs::write(path, data.as_slice()).is_ok();
            Free.unwrap()(data.data as *const c_void);
            res
        }
    }
}

fn load_game_from_slot_detour(slot_name: *const FString, user_index: i32) -> *const USaveGame {
    unsafe {
        let slot_name = &*slot_name;
        if slot_name.to_os_string().to_string_lossy() == "Player" {
            LoadGameFromSlot.call(slot_name, user_index)
        } else if let Some(data) =
            get_path_for_slot(slot_name).and_then(|path| std::fs::read(path).ok())
        {
            LoadGameFromMemory.unwrap()(&TArray::from_slice(data.as_slice()))
        } else {
            std::ptr::null()
        }
    }
}

fn does_save_game_exist_detour(slot_name: *const FString, user_index: i32) -> bool {
    unsafe {
        let slot_name = &*slot_name;
        if slot_name.to_os_string().to_string_lossy() == "Player" {
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
        let name: *mut FString = GetServerName.call(a, b) as *mut _;

        let prefix = "[MODDED] ".encode_utf16().collect::<Vec<_>>();
        let old_num = (*name).num;

        let new_num = (*name).num + prefix.len() as i32;
        if (*name).max < new_num {
            Resize16.unwrap()(name as *const c_void, new_num);
            (*name).max = new_num;
        }
        (*name).num = new_num;

        let memory = (*name).as_slice_mut();

        memory.copy_within(0..old_num as usize, prefix.len());
        memory[0..prefix.len()].copy_from_slice(&prefix);

        name
    }
}
