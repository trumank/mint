use patternsleuth::resolvers::futures::future::join_all;
use patternsleuth::resolvers::unreal::blueprint_library::UFunctionBind;
use patternsleuth::resolvers::unreal::fname::{FNameCtorWchar, FNameToString};
use patternsleuth::resolvers::unreal::game_loop::Main;
use patternsleuth::resolvers::unreal::gmalloc::GMalloc;
use patternsleuth::resolvers::unreal::kismet::{FFrameStep, FFrameStepExplicitProperty};
use patternsleuth::resolvers::unreal::pak::FPakPlatformFileInitialize;
use patternsleuth::resolvers::unreal::save_game::{
    UGameplayStaticsDoesSaveGameExist, UGameplayStaticsLoadGameFromMemory,
    UGameplayStaticsLoadGameFromSlot, UGameplayStaticsSaveGameToMemory,
    UGameplayStaticsSaveGameToSlot,
};
use patternsleuth::resolvers::unreal::*;
use patternsleuth::resolvers::*;
use patternsleuth::scanner::Pattern;
use patternsleuth::MemoryAccessorTrait;

#[cfg(feature = "serde-resolvers")]
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
pub struct GetServerName(pub usize);
impl_resolver_singleton!(collect, GetServerName);
impl_resolver_singleton!(PEImage, GetServerName, |ctx| async {
    let patterns = [
        "48 89 5C 24 10 48 89 6C 24 18 48 89 74 24 20 57 41 56 41 57 48 83 EC 30 45 33 FF 4C 8B F2 48 8B D9 44 89 7C 24 50 41 8B FF"
    ];

    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;

    // matches two but the first is the one we need
    // could potentially get number of xrefs if this becomes problematic
    Ok(Self(
        res.into_iter()
            .flatten()
            .next()
            .context("expected at least one")?,
    ))
});

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
pub struct FOnlineSessionSettingsSetFString(pub usize);
impl_resolver_singleton!(collect, FOnlineSessionSettingsSetFString);
impl_resolver_singleton!(PEImage, FOnlineSessionSettingsSetFString, |ctx| async {
    let patterns = ["48 89 5C 24 ?? 48 89 54 24 ?? 55 56 57 48 83 EC 40 49 8B F8 48 8D 69"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(ensure_one(res.into_iter().flatten())?))
});

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
pub struct USessionHandlingFSDFillSessionSetttingInner(pub usize);
impl_resolver_singleton!(collect, USessionHandlingFSDFillSessionSetttingInner);
impl_resolver_singleton!(
    PEImage,
    USessionHandlingFSDFillSessionSetttingInner,
    |ctx| async {
        let patterns = [
            "48 89 5C 24 ?? 48 89 4C 24 ?? 55 56 57 41 54 41 55 41 56 41 57 48 8B EC 48 81 EC 80 00 00 00 4C 8B FA",
            "48 89 5C 24 ?? 4C 89 4C 24 ?? 48 89 4C 24 ?? 55 56 57 41 54 41 55 41 56 41 57 48 8B EC 48 83 EC 70",
        ];
        let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
        Ok(Self(ensure_one(res.into_iter().flatten())?))
    }
);

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
pub struct ModsFName(pub usize);
impl_resolver_singleton!(collect, ModsFName);
impl_resolver_singleton!(PEImage, ModsFName, |ctx| async {
    let strings = ctx
        .scan(
            Pattern::from_bytes("Mods\0".encode_utf16().flat_map(u16::to_le_bytes).collect())
                .unwrap(),
        )
        .await;

    let refs = join_all(strings.iter().map(|s| {
        ctx.scan(
            Pattern::new(format!(
                "41 b8 01 00 00 00 48 8d 15 X0x{s:X} 48 8d 0d | ?? ?? ?? ?? e9 ?? ?? ?? ??"
            ))
            .unwrap(),
        )
    }))
    .await;

    Ok(Self(try_ensure_one(
        refs.iter()
            .flatten()
            .map(|a| Ok(ctx.image().memory.rip4(*a)?)),
    )?))
});

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
pub struct Disable(pub usize);
impl_resolver_singleton!(collect, Disable);
impl_resolver_singleton!(PEImage, Disable, |ctx| async {
    let patterns = ["4C 8B B4 24 48 01 00 00 0F 84"];

    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;

    Ok(Self(ensure_one(res.into_iter().flatten())?))
});

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
pub struct UObjectTemperatureComponentTimerCallback(pub usize);
impl_resolver_singleton!(collect, UObjectTemperatureComponentTimerCallback);
impl_resolver_singleton!(
    PEImage,
    UObjectTemperatureComponentTimerCallback,
    |ctx| async {
        let patterns = ["40 55 57 41 56 48 8D 6C 24 ?? 48 81 EC 20 01 00 00"];
        let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
        Ok(Self(ensure_one(res.into_iter().flatten())?))
    }
);
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
pub struct ProcessMulticastDelegate(pub usize);
impl_resolver_singleton!(collect, ProcessMulticastDelegate);
impl_resolver_singleton!(PEImage, ProcessMulticastDelegate, |ctx| async {
    let patterns = ["4C 8B DC 57 41 54 41 56 48 81 EC A0 00 00 00"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(ensure_one(res.into_iter().flatten())?))
});

#[derive(Debug, PartialEq)]
#[cfg_attr(
    feature = "serde-resolvers",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct GetAllSpawnPointsInSphere(pub usize);
impl_resolver_singleton!(collect, GetAllSpawnPointsInSphere);
impl_resolver_singleton!(PEImage, GetAllSpawnPointsInSphere, |ctx| async {
    let patterns = ["48 89 75 ?? 48 89 75 ?? E8 | ?? ?? ?? ?? B8 01 00 00 00"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(try_ensure_one(res.iter().flatten().map(
        |a| -> Result<usize> { Ok(ctx.image().memory.rip4(*a)?) },
    ))?))
});

#[derive(Debug, PartialEq)]
#[cfg_attr(
    feature = "serde-resolvers",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct GetPath(pub usize);
impl_resolver_singleton!(collect, GetPath);
impl_resolver_singleton!(PEImage, GetPath, |ctx| async {
    let patterns = ["49 8B CA 48 8D 45 ?? 48 89 44 24 ?? 48 8D 45 ?? 48 89 44 24 ?? E8 | ?? ?? ??"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(try_ensure_one(res.iter().flatten().map(
        |a| -> Result<usize> { Ok(ctx.image().memory.rip4(*a)?) },
    ))?))
});

impl_try_collector! {
    #[derive(Debug, PartialEq)]
    #[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
    pub struct ServerModsResolution {
        pub set_fstring: FOnlineSessionSettingsSetFString,
        pub fill_session_setting: USessionHandlingFSDFillSessionSetttingInner,
        pub mods_fname: ModsFName,
    }
}

impl_try_collector! {
    #[derive(Debug, PartialEq)]
    #[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
    pub struct ServerNameResolution {
        pub get_server_name: GetServerName,
    }
}

impl_try_collector! {
    #[derive(Debug, PartialEq)]
    #[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
    pub struct SaveGameResolution {
        pub save_game_to_memory: UGameplayStaticsSaveGameToMemory,
        pub save_game_to_slot: UGameplayStaticsSaveGameToSlot,
        pub load_game_from_memory: UGameplayStaticsLoadGameFromMemory,
        pub load_game_from_slot: UGameplayStaticsLoadGameFromSlot,
        pub does_save_game_exist: UGameplayStaticsDoesSaveGameExist,
    }
}

impl_try_collector! {
    #[derive(Debug, PartialEq)]
    #[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
    pub struct GasFixResolution {
        pub timer_callback: UObjectTemperatureComponentTimerCallback,
        pub process_multicast_delegate: ProcessMulticastDelegate,
    }
}

impl_try_collector! {
    #[derive(Debug, PartialEq)]
    #[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
    pub struct CoreResolution {
        pub gmalloc: GMalloc,
        pub main: Main,
        pub fnametostring: FNameToString,
        pub fname_ctor_wchar: FNameCtorWchar,
        pub uobject_base_utility_get_path_name: UObjectBaseUtilityGetPathName,
        pub ufunction_bind: UFunctionBind,
        pub fframe_step: FFrameStep,
        pub fframe_step_explicit_property: FFrameStepExplicitProperty,
        pub fpak_platform_file_initialize: FPakPlatformFileInitialize,
    }
}

impl_collector! {
    #[derive(Debug, PartialEq)]
    #[cfg_attr(feature = "serde-resolvers", derive(Serialize, Deserialize))]
    pub struct HookResolution {
        pub disable: Disable,
        pub server_name: ServerNameResolution,
        pub server_mods: ServerModsResolution,
        pub save_game: SaveGameResolution,
        pub gas_fix: GasFixResolution,
        pub core: CoreResolution,
        pub get_path: GetPath,
        pub get_all_spawn_points_in_sphere: GetAllSpawnPointsInSphere,
    }
}
