use std::{ffi::c_void, path::Path};

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

#[no_mangle]
#[allow(non_snake_case, unused_variables)]
extern "system" fn X3DAudioCalculate() {}

#[no_mangle]
#[allow(non_snake_case, unused_variables)]
extern "system" fn X3DAudioInitialize() {}

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

fn scan(data: &[u8], pattern: &[u8]) -> Option<usize> {
    memchr::memchr_iter(pattern[0], &data[0..data.len() - pattern.len() - 1])
        .find(|&i| &data[i..i + pattern.len()] == pattern)
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

    let memory = std::slice::from_raw_parts_mut(
        mod_info.lpBaseOfDll as *mut u8,
        mod_info.SizeOfImage as usize,
    );

    let pattern = [0x4C, 0x8B, 0xB4, 0x24, 0x48, 0x01, 0x00, 0x00, 0x0F, 0x84];
    let Some(sig_rva) = scan(memory, &pattern) else {
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
}
