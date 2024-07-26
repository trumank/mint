use std::ffi::c_void;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::hooks::ExecFn;
use hook_lib::globals;
use hook_lib::ue::{self, FName, FString, TArray, TMap};

retour::static_detour! {
    static GetServerName: unsafe extern "system" fn(*const c_void, *const c_void) -> *const ue::FString;
    static USessionHandlingFSDFillSessionSetting: unsafe extern "system" fn(*const c_void, *mut c_void, bool, *mut c_void, *mut c_void);
}

pub fn kismet_hooks() -> &'static [(&'static str, ExecFn)] {
    &[(
        "/Script/FSD.SessionHandling:FSDGetModsInstalled",
        exec_get_mods_installed as ExecFn,
    )]
}

pub unsafe fn init_hooks() -> Result<()> {
    if let Ok(server_name) = &globals().resolution.server_name {
        GetServerName
            .initialize(
                std::mem::transmute(server_name.get_server_name.0),
                detour_get_server_name,
            )?
            .enable()?;
    }

    if let Ok(server_mods) = &globals().resolution.server_mods {
        USessionHandlingFSDFillSessionSetting
            .initialize(
                std::mem::transmute(server_mods.fill_session_setting.0),
                detour_fill_session_setting,
            )?
            .enable()?;
    }

    Ok(())
}

fn detour_get_server_name(a: *const c_void, b: *const c_void) -> *const ue::FString {
    unsafe {
        let name = GetServerName.call(a, b).cast_mut().as_mut().unwrap();

        let mut new_name = widestring::U16String::new();
        new_name.push_slice([0x5b, 0x4d, 0x4f, 0x44, 0x44, 0x45, 0x44, 0x5d, 0x20]);
        new_name.push_slice(name.as_slice());

        name.clear();
        name.extend_from_slice(new_name.as_slice());
        name.push(0);

        name
    }
}

fn detour_fill_session_setting(
    world: *const c_void,
    game_settings: *mut c_void,
    full_server: bool,
    unknown1: *mut c_void,
    unknown2: *mut c_void,
) {
    unsafe {
        USessionHandlingFSDFillSessionSetting.call(
            world,
            game_settings,
            full_server,
            unknown1,
            unknown2,
        );

        let name = globals().meta.to_server_list_string();

        let s: FString = serde_json::to_string(&vec![JsonMod {
            name,
            version: "mint".into(),
            category: 0,
        }])
        .unwrap()
        .as_str()
        .into();

        type Fn = unsafe extern "system" fn(*const c_void, ue::FName, *const ue::FString, u32);

        let f: Fn = std::mem::transmute(
            globals()
                .resolution
                .server_mods
                .as_ref()
                .unwrap()
                .set_fstring
                .0,
        );

        f(game_settings, ue::FName::new(&"Mods".into()), &s, 3);
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonMod {
    name: String,
    version: String,
    category: i32,
}

#[derive(Debug)]
#[repr(C)]
struct FBlueprintSessionResult {
    online_result: FOnlineSessionSearchResult,
}
#[derive(Debug)]
#[repr(C)]
struct FOnlineSessionSearchResult {
    session: FOnlineSession,
    ping_in_ms: i32,
}
#[derive(Debug)]
#[repr(C)]
struct FOnlineSession {
    vtable: u64,
    owning_user_id: TSharedPtr, // TSharedPtr<FUniqueNetId const ,0> OwningUserId;
    owning_user_name: FString,
    session_settings: FOnlineSessionSettings,
    session_info: TSharedPtr, //class TSharedPtr<FOnlineSessionInfo,0> SessionInfo;
    num_open_private_connections: i32,
    num_open_public_connections: i32,
}

#[derive(Debug)]
#[repr(C)]
struct FOnlineSessionSettings {
    vtable: u64,
    num_public_connections: i32,
    num_private_connections: i32,
    b_should_advertise: u8,
    b_allow_join_in_progress: u8,
    b_is_lan_match: u8,
    b_is_dedicated: u8,
    b_uses_stats: u8,
    b_allow_invites: u8,
    b_uses_presence: u8,
    b_allow_join_via_presence: u8,
    b_allow_join_via_presence_friends_only: u8,
    b_anti_cheat_protected: u8,
    build_unique_id: i32,
    settings: TMap<FName, FOnlineSessionSetting>,
    member_settings: [u64; 10],
}

#[derive(Debug)]
#[repr(C)]
struct FOnlineSessionSetting {
    data: FVariantData,
    padding: [u32; 2],
}

#[derive(Debug)]
#[repr(u32)]
#[allow(unused)]
enum EOnlineKeyValuePairDataType {
    Empty,
    Int32,
    UInt32,
    Int64,
    UInt64,
    Double,
    String,
    Float,
    Blob,
    Bool,
    Json,
    #[allow(clippy::upper_case_acronyms)]
    MAX,
}

#[repr(C)]
struct FVariantData {
    type_: EOnlineKeyValuePairDataType,
    value: FVariantDataValue,
}
impl std::fmt::Debug for FVariantData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_struct("FVariantData");
        dbg.field("type", &self.type_);
        unsafe {
            match self {
                Self {
                    type_: EOnlineKeyValuePairDataType::String,
                    value: FVariantDataValue { as_tchar },
                } => {
                    dbg.field("data", &widestring::U16CStr::from_ptr_str(*as_tchar));
                }
                Self {
                    type_: EOnlineKeyValuePairDataType::UInt32,
                    value: FVariantDataValue { as_uint },
                } => {
                    dbg.field("data", &as_uint);
                }
                Self {
                    type_: EOnlineKeyValuePairDataType::Int32,
                    value: FVariantDataValue { as_int },
                } => {
                    dbg.field("data", &as_int);
                }
                _ => {
                    dbg.field("data", &"<unimplemented type>");
                }
            }
        }
        dbg.finish()
    }
}

#[repr(C)]
union FVariantDataValue {
    as_bool: bool,
    as_int: i32,
    as_uint: u32,
    as_float: f32,
    as_int64: i64,
    as_uint64: u64,
    as_double: f64,
    as_tchar: *const u16,
    as_blob: std::mem::ManuallyDrop<FVariantDataValueBlob>,
}

#[repr(C)]
struct FVariantDataValueBlob {
    blob_data: *const u8,
    blob_size: u32,
}

#[cfg(test)]
mod test {
    use super::*;
    const _: [u8; 0x20] = [0; std::mem::size_of::<FOnlineSessionSetting>()];
    const _: [u8; 0x18] = [0; std::mem::size_of::<FVariantData>()];
    const _: [u8; 0x10] = [0; std::mem::size_of::<FVariantDataValue>()];
    const _: [u8; 0x10] = [0; std::mem::size_of::<FVariantDataValueBlob>()];
}

#[derive(Debug)]
#[repr(C)]
struct TSharedPtr {
    a: u64,
    b: u64,
}

unsafe extern "system" fn exec_get_mods_installed(
    _context: *mut ue::UObject,
    stack: *mut ue::kismet::FFrame,
    result: *mut c_void,
) {
    let stack = stack.as_mut().unwrap();

    let session: FBlueprintSessionResult = stack.arg();
    let _exclude_verified_mods: bool = stack.arg();

    let result = &mut *(result as *mut TArray<FString>);
    result.clear();

    let settings = &session.online_result.session.session_settings.settings;

    let mods = settings.find(FName::new(&"Mods".into()));
    if let Some(mods) = mods {
        if let FVariantData {
            type_: EOnlineKeyValuePairDataType::String,
            value: FVariantDataValue { as_tchar },
        } = mods.data
        {
            if let Ok(string) = widestring::U16CStr::from_ptr_str(as_tchar).to_string() {
                if let Ok(mods) = serde_json::from_str::<Vec<JsonMod>>(&string) {
                    for m in mods {
                        result.push(m.name.as_str().into());
                    }
                }
            }
        }
    }

    // TODO figure out lifetimes of structs from kismet params
    std::mem::forget(session);

    if !stack.code.is_null() {
        stack.code = stack.code.add(1);
    }
}
