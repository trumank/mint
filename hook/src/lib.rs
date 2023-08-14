use std::{ffi::c_void, path::Path};

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
            DLL_PROCESS_ATTACH => patch(),
            DLL_PROCESS_DETACH => (),
            _ => (),
        }

        true
    }
}

fn scan<'a>(data: &'a [u8], pattern: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    memchr::memchr_iter(pattern[0], &data[0..data.len() - pattern.len() - 1])
        .filter(move |&i| &data[i..i + pattern.len()] == pattern)
}

unsafe fn patch() {
    let Some(pak_path) = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|p| p.join("Content/Paks/mods_P.pak"))
    else {
        return;
    };
    if !pak_path.exists() {
        return;
    }

    let Ok(module) = GetModuleHandleA(None) else {
        return;
    };
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

    let pattern = [0x4C, 0x8B, 0xB4, 0x24, 0x48, 0x01, 0x00, 0x00, 0x0F, 0x84];
    let Some(sig_rva) = scan(memory, &pattern).next() else {
        return;
    };

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

    let pattern = [
        0x48, 0x89, 0x5C, 0x24, 0x10, 0x48, 0x89, 0x6C, 0x24, 0x18, 0x48, 0x89, 0x74, 0x24, 0x20,
        0x57, 0x41, 0x56, 0x41, 0x57, 0x48, 0x83, 0xEC, 0x30, 0x45, 0x33, 0xFF, 0x4C, 0x8B, 0xF2,
        0x48, 0x8B, 0xD9, 0x44, 0x89, 0x7C, 0x24, 0x50, 0x41, 0x8B, 0xFF,
    ];

    let mut server_name_scan = scan(memory, &pattern);
    if let Some(rva) = server_name_scan.next() {
        let address = module_addr.add(rva);

        RESIZE16 = Some(std::mem::transmute(address.add(53 + 4).offset(
            i32::from_le_bytes(memory[rva + 53..rva + 53 + 4].try_into().unwrap()) as isize,
        )));

        let target: FnGetServerName = std::mem::transmute(address);
        GetServerName
            .initialize(target, get_server_name_detour)
            .unwrap()
            .enable()
            .unwrap();
    }
}

#[derive(Debug)]
#[repr(C)]
struct FString {
    data: *const u16,
    num: i32,
    max: i32,
}

type FnGetServerName = unsafe extern "system" fn(*const c_void, *const c_void) -> *const FString;
type FnResize16 = unsafe extern "system" fn(*const c_void, new_max: i32);

static_detour! {
    static GetServerName: unsafe extern "system" fn(*const c_void, *const c_void) -> *const FString;
}

static mut RESIZE16: Option<FnResize16> = None;

fn get_server_name_detour(a: *const c_void, b: *const c_void) -> *const FString {
    unsafe {
        let name: *mut FString = GetServerName.call(a, b) as *mut _;

        let prefix = "[MODDED] ".encode_utf16().collect::<Vec<_>>();
        let old_num = (*name).num;

        let new_num = (*name).num + prefix.len() as i32;
        if (*name).max < new_num {
            RESIZE16.unwrap()(name as *const c_void, new_num);
            (*name).max = new_num;
        }
        (*name).num = new_num;

        let memory = std::slice::from_raw_parts_mut((*name).data as *mut _, (*name).num as usize);

        memory.copy_within(0..old_num as usize, prefix.len());
        memory[0..prefix.len()].copy_from_slice(&prefix);

        name
    }
}
