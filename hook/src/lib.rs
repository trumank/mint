use std::{
    ffi::{c_void, OsString},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use retour::static_detour;
use windows::{
    Win32::Foundation::*,
    Win32::System::{
        LibraryLoader::GetModuleHandleA,
        Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS},
        ProcessStatus::{GetModuleInformation, MODULEINFO},
        SystemServices::*,
        Threading::GetCurrentProcess,
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
                patch().ok();
            }
            DLL_PROCESS_DETACH => (),
            _ => (),
        }

        true
    }
}

// TODO refactor crate layout so this can be shared between the hook and integrator
#[derive(Debug)]
pub enum DRGInstallationType {
    Steam,
    Xbox,
}

impl DRGInstallationType {
    pub fn from_exe_path() -> Result<Self> {
        let exe_name = std::env::current_exe()
            .context("could not determine running exe")?
            .file_name()
            .context("failed to get exe path")?
            .to_string_lossy()
            .to_lowercase();
        Ok(match exe_name.as_str() {
            "fsd-win64-shipping.exe" => Self::Steam,
            "fsd-wingdk-shipping.exe" => Self::Xbox,
            _ => bail!("unrecognized exe file name: {exe_name}"),
        })
    }
}

fn scan<'a>(data: &'a [u8], pattern: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    memchr::memchr_iter(pattern[0], &data[0..data.len() - pattern.len() - 1])
        .filter(move |&i| &data[i..i + pattern.len()] == pattern)
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
    if !pak_path.exists() {
        return Ok(());
    }

    let installation_type = DRGInstallationType::from_exe_path()?;

    let module = GetModuleHandleA(None).context("could not find main module")?;
    let process = GetCurrentProcess();

    let mut mod_info = MODULEINFO::default();
    GetModuleInformation(
        process,
        module,
        &mut mod_info as *mut _,
        std::mem::size_of::<MODULEINFO>() as u32,
    );

    let module_addr = mod_info.lpBaseOfDll;

    let memory = std::slice::from_raw_parts_mut(
        mod_info.lpBaseOfDll as *mut u8,
        mod_info.SizeOfImage as usize,
    );

    let pattern = [
        0x48, 0x89, 0x5C, 0x24, 0x10, 0x48, 0x89, 0x6C, 0x24, 0x18, 0x48, 0x89, 0x74, 0x24, 0x20,
        0x57, 0x41, 0x56, 0x41, 0x57, 0x48, 0x83, 0xEC, 0x30, 0x45, 0x33, 0xFF, 0x4C, 0x8B, 0xF2,
        0x48, 0x8B, 0xD9, 0x44, 0x89, 0x7C, 0x24, 0x50, 0x41, 0x8B, 0xFF,
    ];

    {
        let mut server_name_scan = scan(memory, &pattern);
        if let Some(rva) = server_name_scan.next() {
            let address = module_addr.add(rva);

            Resize16 = Some(std::mem::transmute(address.add(53 + 4).offset(
                i32::from_le_bytes(memory[rva + 53..rva + 53 + 4].try_into().unwrap()) as isize,
            )));

            let target: FnGetServerName = std::mem::transmute(address);
            GetServerName
                .initialize(target, get_server_name_detour)?
                .enable()?;
        }
    }

    if matches!(installation_type, DRGInstallationType::Steam) {
        let pattern = [0x4C, 0x8B, 0xB4, 0x24, 0x48, 0x01, 0x00, 0x00, 0x0F, 0x84];
        let mut scan_iter = scan(memory, &pattern);
        if let Some(sig_rva) = scan_iter.next() {
            drop(scan_iter);

            let patch = [0xB8, 0x01, 0x00, 0x00, 0x00];

            let rva = sig_rva + 29;
            let patch_mem = &mut memory[rva..rva + 5];

            let mut old: PAGE_PROTECTION_FLAGS = Default::default();
            VirtualProtect(
                patch_mem.as_ptr() as *const c_void,
                patch_mem.len(),
                PAGE_EXECUTE_READWRITE,
                &mut old as *mut _,
            );

            patch_mem.copy_from_slice(&patch);

            VirtualProtect(
                patch_mem.as_ptr() as *const c_void,
                patch_mem.len(),
                old,
                &mut old as *mut _,
            );
        }
    }
    if matches!(installation_type, DRGInstallationType::Xbox) {
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

        // SaveGameToSlot
        let pattern = [
            0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xec,
            0x40, 0x48, 0x8b, 0xda, 0x33, 0xf6, 0x48, 0x8d, 0x54, 0x24, 0x30, 0x48, 0x89, 0x74,
            0x24, 0x30, 0x48, 0x89, 0x74, 0x24, 0x38, 0x41, 0x8b, 0xf8,
        ];

        let mut scan_iter = scan(memory, &pattern);
        if let Some(rva) = scan_iter.next() {
            let address = module_addr.add(rva);

            SaveGameToMemory = Some(std::mem::transmute(address.add(39 + 4).offset(
                i32::from_le_bytes(memory[rva + 39..rva + 39 + 4].try_into().unwrap()) as isize,
            )));

            let target: FnSaveGameToSlot = std::mem::transmute(address);
            SaveGameToSlot
                .initialize(target, save_game_to_slot_detour)?
                .enable()?;
        }

        // LoadGameFromMemory
        let pattern = [
            0x40, 0x55, 0x48, 0x8d, 0xac, 0x24, 0x00, 0xff, 0xff, 0xff, 0x48, 0x81, 0xec, 0x00,
            0x02, 0x00, 0x00, 0x83, 0x79, 0x08, 0x00,
        ];
        let mut scan_iter = scan(memory, &pattern);
        if let Some(rva) = scan_iter.next() {
            let address = module_addr.add(rva);

            LoadGameFromMemory = Some(std::mem::transmute(address));

            // LoadGameFromSlot
            let pattern = [
                0x48, 0x8b, 0xc4, 0x55, 0x57, 0x48, 0x8d, 0xa8, 0xd8, 0xfe, 0xff, 0xff, 0x48, 0x81,
                0xec, 0x18, 0x02, 0x00, 0x00,
            ];
            let mut scan_iter = scan(memory, &pattern);
            if let Some(rva) = scan_iter.next() {
                let address = module_addr.add(rva);

                let target: FnLoadGameFromSlot = std::mem::transmute(address);
                LoadGameFromSlot
                    .initialize(target, load_game_from_slot_detour)?
                    .enable()?;
            }
        }
    }
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
        use std::os::windows::ffi::OsStringExt;
        let slice = self.as_slice();
        let len = slice
            .iter()
            .enumerate()
            .find_map(|(i, &b)| (b == 0).then_some(i))
            .unwrap_or(slice.len());
        std::ffi::OsString::from_wide(&slice[0..len])
    }
}

type FnResize16 = unsafe extern "system" fn(*const c_void, new_max: i32);
type FnGetServerName = unsafe extern "system" fn(*const c_void, *const c_void) -> *const FString;
type FnSaveGameToSlot = unsafe extern "system" fn(*const USaveGame, *const FString, i32) -> bool;
type FnSaveGameToMemory = unsafe extern "system" fn(*const USaveGame, *mut TArray<u8>) -> bool;
type FnLoadGameFromSlot = unsafe extern "system" fn(*const FString, i32) -> *const USaveGame;
type FnLoadGameFromMemory = unsafe extern "system" fn(*const TArray<u8>) -> *const USaveGame;

static_detour! {
    static GetServerName: unsafe extern "system" fn(*const c_void, *const c_void) -> *const FString;
    static SaveGameToSlot: unsafe extern "system" fn(*const USaveGame, *const FString, i32) -> bool;
    static LoadGameFromSlot: unsafe extern "system" fn(*const FString, i32) -> *const USaveGame;
}

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

            std::fs::write(path, data.as_slice()).is_ok()
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
            // TODO this currently leaks the buffer but to free it we need to find the allocator
            LoadGameFromMemory.unwrap()(&TArray::from_slice(data.as_slice()))
        } else {
            std::ptr::null()
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
