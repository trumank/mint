use patternsleuth::resolvers::futures::future::join_all;
use patternsleuth::resolvers::unreal::blueprint_library::UFunctionBind;
use patternsleuth::resolvers::unreal::fname::FNameToString;
use patternsleuth::resolvers::unreal::gmalloc::GMalloc;
use patternsleuth::resolvers::unreal::kismet::{FFrameStep, FFrameStepExplicitProperty};
use patternsleuth::resolvers::unreal::save_game::{
    UGameplayStaticsDoesSaveGameExist, UGameplayStaticsLoadGameFromMemory,
    UGameplayStaticsLoadGameFromSlot, UGameplayStaticsSaveGameToMemory,
    UGameplayStaticsSaveGameToSlot,
};
use patternsleuth::resolvers::unreal::*;
use patternsleuth::resolvers::*;
use patternsleuth::scanner::Pattern;
use patternsleuth::MemoryAccessorTrait;

#[derive(Debug, PartialEq)]
pub struct GetServerName(pub usize);
impl_resolver_singleton!(GetServerName, |ctx| async {
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
pub struct FOnlineSessionSettingsSetFString(pub usize);
impl_resolver_singleton!(FOnlineSessionSettingsSetFString, |ctx| async {
    let patterns = ["48 89 5C 24 ?? 48 89 54 24 ?? 55 56 57 48 83 EC 40 49 8B F8 48 8D 69"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(ensure_one(res.into_iter().flatten())?))
});

#[derive(Debug, PartialEq)]
pub struct USessionHandlingFSDFillSessionSettting(pub usize);
impl_resolver_singleton!(USessionHandlingFSDFillSessionSettting, |ctx| async {
    let patterns = ["48 89 5C 24 ?? 48 89 74 24 ?? 48 89 7C 24 ?? 55 41 54 41 55 41 56 41 57 48 8B EC 48 83 EC 50 48 8B B9"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(ensure_one(res.into_iter().flatten())?))
});

#[derive(Debug, PartialEq)]
pub struct ModsFName(pub usize);
impl_resolver_singleton!(ModsFName, |ctx| async {
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
pub struct SemicolonHReplace(pub usize);
impl_resolver_singleton!(SemicolonHReplace, |ctx| async {
    let patterns = ["48 8B CB 48 8D 55 E0 E8 ?? ?? ?? ?? 4D 63 FE 48 8B F0 45 8D 77 01 44 89 75 B8 45 3B F4 7E 18 41 8B D7 48 8D 4D B0 E8 ?? ?? ?? ?? 44 8B 65 BC 44 8B 75 B8 4C 8B 6D B0 33 D2 49 8B CF 48 C1 E1 04 49 03 CD 48 89 11 48 8B 06 48 89 01 48 89 16 8B 46 08 89 41 08 8B 46 0C 89 41 0C 48 89 56 08 48 8B 4D E0 48 85 C9 74 05 E8"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(ensure_one(res.into_iter().flatten())?))
});

#[derive(Debug, PartialEq)]
pub struct Disable(pub usize);
impl_resolver_singleton!(Disable, |ctx| async {
    let patterns = ["4C 8B B4 24 48 01 00 00 0F 84"];

    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;

    Ok(Self(ensure_one(res.into_iter().flatten())?))
});

#[derive(Debug, PartialEq)]
pub struct UObjectTemperatureComponentTimerCallback(pub usize);
impl_resolver_singleton!(UObjectTemperatureComponentTimerCallback, |ctx| async {
    let patterns = ["40 55 57 41 56 48 8D 6C 24 ?? 48 81 EC 20 01 00 00"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(ensure_one(res.into_iter().flatten())?))
});
#[derive(Debug, PartialEq)]
pub struct ProcessMulticastDelegate(pub usize);
impl_resolver_singleton!(ProcessMulticastDelegate, |ctx| async {
    let patterns = ["4C 8B DC 57 41 54 41 56 48 81 EC A0 00 00 00"];
    let res = join_all(patterns.iter().map(|p| ctx.scan(Pattern::new(p).unwrap()))).await;
    Ok(Self(ensure_one(res.into_iter().flatten())?))
});

impl_try_collector! {
    #[derive(Debug, PartialEq)]
    pub struct ServerModsResolution {
        pub set_fstring: FOnlineSessionSettingsSetFString,
        pub fill_session_setting: USessionHandlingFSDFillSessionSettting,
        pub mods_fname: ModsFName,
        pub semicolon_h_replace: SemicolonHReplace,
    }
}

impl_try_collector! {
    #[derive(Debug, PartialEq)]
    pub struct ServerNameResolution {
        pub get_server_name: GetServerName,
    }
}

impl_try_collector! {
    #[derive(Debug, PartialEq)]
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
    pub struct GasFixResolution {
        pub timer_callback: UObjectTemperatureComponentTimerCallback,
        pub process_multicast_delegate: ProcessMulticastDelegate,
    }
}

impl_try_collector! {
    #[derive(Debug, PartialEq)]
    pub struct CoreResolution {
        pub gmalloc: GMalloc,
        pub fnametostring: FNameToString,
        pub uobject_base_utility_get_path_name: UObjectBaseUtilityGetPathName,
        pub ufunction_bind: UFunctionBind,
        pub fframe_step: FFrameStep,
        pub fframe_step_explicit_property: FFrameStepExplicitProperty,
    }
}

impl_collector! {
    #[derive(Debug, PartialEq)]
    pub struct HookResolution {
        pub disable: Disable,
        pub server_name: ServerNameResolution,
        pub server_mods: ServerModsResolution,
        pub save_game: SaveGameResolution,
        pub gas_fix: GasFixResolution,
        pub core: CoreResolution,
    }
}
