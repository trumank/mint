use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::{self, BufReader, BufWriter, Cursor, ErrorKind, Read, Seek};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use repak::PakWriter;
use tracing::info;

use crate::providers::{ModInfo, ReadSeek};
use crate::splice::TrackedStatement;
use crate::{open_file, splice, DRGInstallation};

use unreal_asset::{
    exports::ExportBaseTrait,
    flags::EObjectFlags,
    kismet::{
        EExprToken, ExByteConst, ExCallMath, ExLet, ExLetObj, ExLocalVariable, ExRotationConst,
        ExSelf, ExSoftObjectConst, ExStringConst, ExVectorConst, FieldPath, KismetPropertyPointer,
    },
    kismet::{ExFalse, KismetExpression},
    properties::object_property::TopLevelAssetPath,
    properties::{
        int_property::BoolProperty,
        object_property::{SoftObjectPath, SoftObjectProperty},
        str_property::StrProperty,
        struct_property::StructProperty,
        Property,
    },
    types::vector::Vector,
    types::{fname::FName, PackageIndex},
    unversioned::ancestry::Ancestry,
    Asset,
};

/// Why does the uninstall function require a list of Modio mod IDs?
/// Glad you ask. The official integration enables *every mod the user has installed* once it gets
/// re-enabled. We do the user a favor and collect all the installed mods and explicitly add them
/// back to the config so they will be disabled when the game is launched again. Since we have
/// Modio IDs anyway, with just a little more effort we can make the 'uninstall' button work as an
/// 'install' button for the official integration. Best anti-feature ever.
#[tracing::instrument(level = "debug", skip(path_pak))]
pub fn uninstall<P: AsRef<Path>>(path_pak: P, modio_mods: HashSet<u32>) -> Result<()> {
    let installation = DRGInstallation::from_pak_path(path_pak)?;
    let path_mods_pak = installation.paks_path().join("mods_P.pak");
    let path_hook_dll = installation.binaries_directory().join("x3daudio1_7.dll");
    match std::fs::remove_file(&path_mods_pak) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
    .with_context(|| format!("failed to remove {}", path_mods_pak.display()))?;
    match std::fs::remove_file(&path_hook_dll) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
    .with_context(|| format!("failed to remove {}", path_hook_dll.display()))?;
    uninstall_modio(&installation, modio_mods).ok();
    Ok(())
}

#[tracing::instrument(level = "debug")]
fn uninstall_modio(installation: &DRGInstallation, modio_mods: HashSet<u32>) -> Result<()> {
    #[derive(Debug, serde::Deserialize)]
    struct ModioState {
        #[serde(rename = "Mods")]
        mods: Vec<ModioMod>,
    }
    #[derive(Debug, serde::Deserialize)]
    struct ModioMod {
        #[serde(rename = "ID")]
        id: u32,
    }
    let Some(modio_dir) = installation.modio_directory() else {
        return Ok(());
    };
    let modio_state: ModioState = serde_json::from_reader(std::io::BufReader::new(
        std::fs::File::open(modio_dir.join("metadata/state.json"))?,
    ))?;
    let config_path = installation
        .root
        .join("Saved/Config/WindowsNoEditor/GameUserSettings.ini");
    let mut config = ini::Ini::load_from_file(&config_path)?;

    let ignore_keys = HashSet::from(["CurrentModioUserId"]);

    config
        .entry(Some("/Script/FSD.UserGeneratedContent".to_string()))
        .or_insert_with(Default::default);
    if let Some(ugc_section) = config.section_mut(Some("/Script/FSD.UserGeneratedContent")) {
        let local_mods = installation
            .root
            .join("Mods")
            .read_dir()?
            .map(|f| {
                let f = f?;
                Ok((!f.path().is_file())
                    .then_some(f.file_name().to_string_lossy().to_string().to_string()))
            })
            .collect::<Result<Vec<Option<String>>>>()?;
        let to_remove = HashSet::from_iter(ugc_section.iter().map(|(k, _)| k))
            .difference(&ignore_keys)
            .map(|&k| k.to_owned())
            .collect::<Vec<String>>();
        for r in to_remove {
            let _ = ugc_section.remove_all(r);
        }
        for m in modio_state.mods {
            ugc_section.insert(
                m.id.to_string(),
                if modio_mods.contains(&m.id) {
                    "True"
                } else {
                    "False"
                },
            );
        }
        for m in local_mods.into_iter().flatten() {
            ugc_section.insert(m, "False");
        }
        ugc_section.insert("CheckGameversion", "False");
    }

    config.write_to_file_opt(
        config_path,
        ini::WriteOption {
            line_separator: ini::LineSeparator::CRLF,
            ..Default::default()
        },
    )?;
    Ok(())
}

pub fn integrate<P: AsRef<Path>>(path_pak: P, mods: Vec<(ModInfo, PathBuf)>) -> Result<()> {
    let installation = DRGInstallation::from_pak_path(&path_pak)?;
    let path_mod_pak = installation.paks_path().join("mods_P.pak");
    let path_hook_dll = installation.binaries_directory().join("x3daudio1_7.dll");

    let mut fsd_pak_reader = BufReader::new(open_file(path_pak)?);
    let fsd_pak = repak::PakReader::new_any(&mut fsd_pak_reader, None)?;

    #[derive(Debug, Default)]
    struct Dir<'a> {
        name: &'a OsStr,
        children: HashMap<OsString, Dir<'a>>,
    }
    let paths = fsd_pak
        .files()
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let mut directories: HashMap<OsString, Dir> = HashMap::new();
    for f in &paths {
        let mut dir = &mut directories;
        for c in f.components() {
            dir = &mut dir
                .entry(c.as_os_str().to_ascii_lowercase())
                .or_insert(Dir {
                    name: c.as_os_str(),
                    children: Default::default(),
                })
                .children;
        }
    }

    #[derive(Debug, Default)]
    struct RawAsset {
        uasset: Option<Vec<u8>>,
        uexp: Option<Vec<u8>>,
    }
    impl RawAsset {
        fn parse(&self) -> Result<Asset<Cursor<&Vec<u8>>>> {
            Ok(unreal_asset::Asset::new(
                Cursor::new(self.uasset.as_ref().unwrap()),
                Some(Cursor::new(self.uexp.as_ref().unwrap())),
                unreal_asset::engine_version::EngineVersion::VER_UE4_27,
                None,
            )?)
        }
    }

    // Used to normalize match path case to existing files in the DRG pak.
    let normalize_path = |path_str: &str| {
        let mut dir = Some(&directories);
        let path = Path::new(path_str);
        let mut normalized_path = PathBuf::new();
        for c in path.components() {
            if let Some(entry) = dir.and_then(|d| d.get(&c.as_os_str().to_ascii_lowercase())) {
                normalized_path.push(entry.name);
                dir = Some(&entry.children);
            } else {
                normalized_path.push(c);
            }
        }
        normalized_path
    };

    let write_file = |pak: &mut PakWriter<_>, data: &[u8], path: &str| -> Result<()> {
        let binding = normalize_path(path);
        let path = binding.to_str().unwrap().replace('\\', "/");

        pak.write_file(&path, &mut Cursor::new(data))?;

        Ok(())
    };

    let write_asset = |pak: &mut PakWriter<_>, asset: Asset<_>, path: &str| -> Result<()> {
        let mut data_out = (Cursor::new(vec![]), Cursor::new(vec![]));

        asset.write_data(&mut data_out.0, Some(&mut data_out.1))?;
        data_out.0.rewind()?;
        data_out.1.rewind()?;

        write_file(pak, &data_out.0.into_inner(), &format!("{path}.uasset"))?;
        write_file(pak, &data_out.1.into_inner(), &format!("{path}.uexp"))?;

        Ok(())
    };

    fn format_soft_class(path: &Path) -> String {
        let name = path.file_stem().unwrap().to_string_lossy();
        format!(
            "/Game/{}{}_C",
            path.strip_prefix("FSD/Content")
                .unwrap()
                .to_string_lossy()
                .strip_suffix("uasset")
                .unwrap(),
            name
        )
    }

    let pcb_path = "FSD/Content/Game/BP_PlayerControllerBase";
    let patch_paths = [
        "FSD/Content/Game/BP_GameInstance",
        "FSD/Content/Game/SpaceRig/BP_PlayerController_SpaceRig",
        "FSD/Content/Game/StartMenu/Bp_StartMenu_PlayerController",
        "FSD/Content/UI/Menu_DeepDives/ITM_DeepDives_Join",
        "FSD/Content/UI/Menu_ServerList/_MENU_ServerList",
        "FSD/Content/UI/Menu_ServerList/WND_JoiningModded",
    ];

    let mut deferred_assets: HashMap<&str, RawAsset> = HashMap::from_iter(
        [pcb_path]
            .iter()
            .chain(patch_paths.iter())
            .map(|path| (*path, RawAsset::default())),
    );

    // collect assets from game pak file
    for (path, asset) in &mut deferred_assets {
        // TODO repak should return an option...
        asset.uasset = match fsd_pak.get(&format!("{path}.uasset"), &mut fsd_pak_reader) {
            Ok(file) => Ok(Some(file)),
            Err(repak::Error::MissingEntry(_)) => Ok(None),
            Err(e) => Err(e),
        }?;
        asset.uexp = match fsd_pak.get(&format!("{path}.uexp"), &mut fsd_pak_reader) {
            Ok(file) => Ok(Some(file)),
            Err(repak::Error::MissingEntry(_)) => Ok(None),
            Err(e) => Err(e),
        }?;
    }

    let mut mod_pak = repak::PakWriter::new(
        BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path_mod_pak)?,
        ),
        None,
        repak::Version::V11,
        "../../../".to_string(),
        None,
    );

    let hook_dll = include_bytes!(env!("CARGO_CDYLIB_FILE_HOOK_x3daudio1_7"));
    if path_hook_dll
        .metadata()
        .map(|m| m.len() != hook_dll.len() as u64)
        .unwrap_or(true)
    {
        std::fs::write(&path_hook_dll, hook_dll)
            .with_context(|| format!("failed to write hook to {}", path_hook_dll.display()))?;
    }

    let mut init_spacerig_assets = HashSet::new();
    let mut init_cave_assets = HashSet::new();

    let mut added_paths = HashSet::new();

    let mods = mods
        .into_iter()
        .map(|(m, path)| {
            let mut buf = get_pak_from_data(Box::new(BufReader::new(open_file(path)?)))?;
            let pak = repak::PakReader::new_any(&mut buf, None)?;

            let mount = Path::new(pak.mount_point());

            for p in pak.files() {
                let j = mount.join(&p);
                let new_path = j
                    .strip_prefix("../../../")
                    .context("prefix does not match")?;
                let new_path_str = &new_path.to_string_lossy().replace('\\', "/");
                let lowercase = new_path_str.to_ascii_lowercase();
                if added_paths.contains(&lowercase) {
                    continue;
                }

                if let Some(filename) = new_path.file_name() {
                    if filename == "AssetRegistry.bin" {
                        continue;
                    }
                    if new_path.extension().and_then(std::ffi::OsStr::to_str)
                        == Some("ushaderbytecode")
                    {
                        continue;
                    }
                    let lower = filename.to_string_lossy().to_lowercase();
                    if lower == "initspacerig.uasset" {
                        init_spacerig_assets.insert(format_soft_class(new_path));
                    }
                    if lower == "initcave.uasset" {
                        init_cave_assets.insert(format_soft_class(new_path));
                    }
                }

                let file_data = pak.get(&p, &mut buf)?;
                if let Some(raw) = new_path_str
                    .strip_suffix(".uasset")
                    .and_then(|path| deferred_assets.get_mut(path))
                {
                    raw.uasset = Some(file_data);
                } else if let Some(raw) = new_path_str
                    .strip_suffix(".uexp")
                    .and_then(|path| deferred_assets.get_mut(path))
                {
                    raw.uexp = Some(file_data);
                } else {
                    write_file(&mut mod_pak, &file_data, new_path_str)?;
                    added_paths.insert(lowercase);
                }
            }
            Ok(m.resolution.url) // TODO don't leak paths of local mods to clients
        })
        .collect::<Result<Vec<String>>>()?;

    {
        let mut pcb_asset = deferred_assets[&pcb_path].parse()?;
        hook_pcb(&mut pcb_asset);
        write_asset(&mut mod_pak, pcb_asset, pcb_path)?;
    }
    for patch_path in patch_paths {
        let mut asset = deferred_assets[&patch_path].parse()?;
        patch(&mut asset)?;
        write_asset(&mut mod_pak, asset, patch_path)?;
    }

    let mut int_pak_reader = Cursor::new(include_bytes!("../integration.pak"));
    let int_pak = repak::PakReader::new_any(&mut int_pak_reader, None)?;

    let mount = Path::new(int_pak.mount_point());
    let files = int_pak.files();
    let mut int_files = files
        .iter()
        .map(|p| {
            (
                mount
                    .join(p)
                    .strip_prefix("../../../")
                    .expect("prefix does not match")
                    .to_string_lossy()
                    .replace('\\', "/"),
                p,
            )
        })
        .collect::<HashMap<_, _>>();

    let int_path = (
        "FSD/Content/_AssemblyStorm/ModIntegration/MI_SpawnMods.uasset",
        "FSD/Content/_AssemblyStorm/ModIntegration/MI_SpawnMods.uexp",
    );

    int_files.remove(int_path.0);
    int_files.remove(int_path.1);

    for (p, new_path) in int_files {
        write_file(
            &mut mod_pak,
            &int_pak.get(&p, &mut int_pak_reader)?,
            new_path,
        )?;
    }

    let mut int_asset = unreal_asset::Asset::new(
        Cursor::new(int_pak.get(int_path.0, &mut int_pak_reader)?),
        Some(Cursor::new(int_pak.get(int_path.1, &mut int_pak_reader)?)),
        unreal_asset::engine_version::EngineVersion::VER_UE4_27,
        None,
    )?;

    inject_init_actors(
        &mut int_asset,
        init_spacerig_assets,
        init_cave_assets,
        &[], //&mods,
    );

    let mut int_out = (Cursor::new(vec![]), Cursor::new(vec![]));

    int_asset.write_data(&mut int_out.0, Some(&mut int_out.1))?;
    int_out.0.rewind()?;
    int_out.1.rewind()?;

    write_file(&mut mod_pak, &mut int_out.0.into_inner(), int_path.0)?;
    write_file(&mut mod_pak, &mut int_out.1.into_inner(), int_path.1)?;

    mod_pak.write_index()?;

    info!(
        "{} mods installed to {}",
        mods.len(),
        path_mod_pak.display()
    );

    Ok(())
}

fn get_pak_from_data(mut data: Box<dyn ReadSeek>) -> Result<Box<dyn ReadSeek>> {
    if let Ok(mut archive) = zip::ZipArchive::new(&mut data) {
        (0..archive.len())
            .map(|i| -> Result<Option<Box<dyn ReadSeek>>> {
                let mut file = archive.by_index(i)?;
                match file.enclosed_name() {
                    Some(p) => {
                        if file.is_file() && p.extension().filter(|e| e == &"pak").is_some() {
                            let mut buf = vec![];
                            file.read_to_end(&mut buf)?;
                            Ok(Some(Box::new(Cursor::new(buf))))
                        } else {
                            Ok(None)
                        }
                    }
                    None => Ok(None),
                }
            })
            .find_map(|e| e.transpose())
            .context("Zip does not contain pak")?
    } else {
        data.rewind()?;
        Ok(data)
    }
}

type ImportChain<'a> = Vec<Import<'a>>;

struct Import<'a> {
    class_package: &'a str,
    class_name: &'a str,
    object_name: &'a str,
}
impl<'a> Import<'a> {
    fn new(class_package: &'a str, class_name: &'a str, object_name: &'a str) -> Import<'a> {
        Import {
            class_package,
            class_name,
            object_name,
        }
    }
}

fn get_import<R: Read + Seek>(asset: &mut Asset<R>, import: ImportChain) -> PackageIndex {
    let mut pi = PackageIndex::new(0);
    for i in import {
        let ai = &asset
            .imports
            .iter()
            .enumerate()
            .find(|(_, ai)| {
                ai.class_package.get_content(|n| n == i.class_package)
                    && ai.class_name.get_content(|n| n == i.class_name)
                    && ai.object_name.get_content(|n| n == i.object_name)
                    && ai.outer_index == pi
            })
            .map(|(index, _)| PackageIndex::from_import(index as i32).unwrap());
        pi = ai.unwrap_or_else(|| {
            let new_import = unreal_asset::Import::new(
                asset.add_fname(i.class_package),
                asset.add_fname(i.class_name),
                pi,
                asset.add_fname(i.object_name),
                false,
            );
            asset.add_import(new_import)
        });
    }
    pi
}

fn find_export_named<'a, R: io::Read + io::Seek>(
    asset: &'a mut unreal_asset::Asset<R>,
    name: &'a str,
) -> Option<&'a mut unreal_asset::exports::normal_export::NormalExport> {
    for export in &mut asset.asset_data.exports {
        if let unreal_asset::exports::Export::NormalExport(export) = export {
            if export.base_export.object_name.get_content(|n| n == name) {
                return Some(export);
            }
        }
    }
    None
}
fn find_array_property_named<'a>(
    export: &'a mut unreal_asset::exports::normal_export::NormalExport,
    name: &'a str,
) -> Option<(
    usize,
    &'a mut unreal_asset::properties::array_property::ArrayProperty,
)> {
    for (i, prop) in &mut export.properties.iter_mut().enumerate() {
        if let unreal_asset::properties::Property::ArrayProperty(prop) = prop {
            if prop.name.get_content(|n| n == name) {
                return Some((i, prop));
            }
        }
    }
    None
}

/// "it's only 3 instructions"
/// "how much boilerplate could there possibly be"
fn hook_pcb<R: Read + Seek>(asset: &mut Asset<R>) {
    let transform = get_import(
        asset,
        vec![
            Import::new("/Script/CoreUObject", "Package", "/Script/CoreUObject"),
            Import::new("/Script/CoreUObject", "ScriptStruct", "Transform"),
        ],
    );
    let actor = get_import(
        asset,
        vec![
            Import::new("/Script/CoreUObject", "Package", "/Script/Engine"),
            Import::new("/Script/CoreUObject", "Class", "Actor"),
        ],
    );
    let load_class = get_import(
        asset,
        vec![
            Import::new("/Script/CoreUObject", "Package", "/Script/Engine"),
            Import::new("/Script/CoreUObject", "Class", "KismetSystemLibrary"),
            Import::new("/Script/CoreUObject", "Function", "LoadClassAsset_Blocking"),
        ],
    );
    let make_transform = get_import(
        asset,
        vec![
            Import::new("/Script/CoreUObject", "Package", "/Script/Engine"),
            Import::new("/Script/CoreUObject", "Class", "KismetMathLibrary"),
            Import::new("/Script/CoreUObject", "Function", "MakeTransform"),
        ],
    );
    let begin_spawning = get_import(
        asset,
        vec![
            Import::new("/Script/CoreUObject", "Package", "/Script/Engine"),
            Import::new("/Script/CoreUObject", "Class", "GameplayStatics"),
            Import::new(
                "/Script/CoreUObject",
                "Function",
                "BeginDeferredActorSpawnFromClass",
            ),
        ],
    );
    let finish_spawning = get_import(
        asset,
        vec![
            Import::new("/Script/CoreUObject", "Package", "/Script/Engine"),
            Import::new("/Script/CoreUObject", "Class", "GameplayStatics"),
            Import::new("/Script/CoreUObject", "Function", "FinishSpawningActor"),
        ],
    );
    let ex_transform = ExCallMath {
        token: EExprToken::ExCallMath,
        stack_node: make_transform,
        parameters: vec![
            ExVectorConst {
                token: EExprToken::ExVectorConst,
                value: unreal_asset::types::vector::Vector::new(
                    0f64.into(),
                    0f64.into(),
                    0f64.into(),
                ),
            }
            .into(),
            ExRotationConst {
                token: EExprToken::ExVectorConst,
                rotator: Vector::new(0f64.into(), 0f64.into(), 0f64.into()),
            }
            .into(),
            ExVectorConst {
                token: EExprToken::ExVectorConst,
                value: unreal_asset::types::vector::Vector::new(
                    1f64.into(),
                    1f64.into(),
                    1f64.into(),
                ),
            }
            .into(),
        ],
    };
    let prop_class_name = asset.add_fname("begin_spawn");
    let prop_class = unreal_asset::fproperty::FObjectProperty {
        generic_property: unreal_asset::fproperty::FGenericProperty {
            name: prop_class_name.clone(),
            flags: EObjectFlags::RF_PUBLIC,
            array_dim: unreal_asset::enums::EArrayDim::TArray,
            element_size: 8,
            property_flags: unreal_asset::flags::EPropertyFlags::CPF_NONE,
            rep_index: 0,
            rep_notify_func: asset.add_fname("None"),
            blueprint_replication_condition: unreal_asset::enums::ELifetimeCondition::CondNone,
            serialized_type: Some(asset.add_fname("ClassProperty")),
        },
        property_class: actor,
    };
    let prop_transform_name = asset.add_fname("transform");
    let prop_transform = unreal_asset::fproperty::FStructProperty {
        generic_property: unreal_asset::fproperty::FGenericProperty {
            name: prop_transform_name.clone(),
            flags: EObjectFlags::RF_PUBLIC,
            array_dim: unreal_asset::enums::EArrayDim::TArray,
            element_size: 48,
            property_flags: unreal_asset::flags::EPropertyFlags::CPF_NONE,
            rep_index: 0,
            rep_notify_func: asset.add_fname("None"),
            blueprint_replication_condition: unreal_asset::enums::ELifetimeCondition::CondNone,
            serialized_type: Some(asset.add_fname("StructProperty")),
        },
        struct_value: transform,
    };
    let prop_begin_spawn_name = asset.add_fname("begin_spawn");
    let prop_begin_spawn = unreal_asset::fproperty::FObjectProperty {
        generic_property: unreal_asset::fproperty::FGenericProperty {
            name: prop_begin_spawn_name.clone(),
            flags: EObjectFlags::RF_PUBLIC,
            array_dim: unreal_asset::enums::EArrayDim::TArray,
            element_size: 8,
            property_flags: unreal_asset::flags::EPropertyFlags::CPF_NONE,
            rep_index: 0,
            rep_notify_func: asset.add_fname("None"),
            blueprint_replication_condition: unreal_asset::enums::ELifetimeCondition::CondNone,
            serialized_type: Some(asset.add_fname("ObjectProperty")),
        },
        property_class: actor,
    };

    let (fi, func) = asset
        .asset_data
        .exports
        .iter_mut()
        .enumerate()
        .find_map(|(i, e)| {
            if let unreal_asset::exports::Export::FunctionExport(func) = e {
                if func
                    .get_base_export()
                    .object_name
                    .get_content(|n| n == "ReceiveBeginPlay")
                {
                    return Some((PackageIndex::from_export(i as i32).unwrap(), func));
                }
            }
            None
        })
        .unwrap();

    func.struct_export.loaded_properties.push(prop_class.into());
    func.struct_export
        .loaded_properties
        .push(prop_transform.into());
    func.struct_export
        .loaded_properties
        .push(prop_begin_spawn.into());
    let inst = func.struct_export.script_bytecode.as_mut().unwrap();
    inst.insert(
        0,
        ExLetObj {
            token: EExprToken::ExLetObj,
            variable_expression: Box::new(
                ExLocalVariable {
                    token: EExprToken::ExLocalVariable,
                    variable: KismetPropertyPointer {
                        old: None,
                        new: Some(FieldPath {
                            path: vec![prop_class_name.clone()],
                            resolved_owner: fi,
                        }),
                    },
                }
                .into(),
            ),
            assignment_expression: Box::new(
                ExCallMath {
                    token: EExprToken::ExCallMath,
                    stack_node: load_class,
                    parameters: vec![
                        ExSoftObjectConst {
                            token: EExprToken::ExSoftObjectConst,
                            value: Box::new(
                                ExStringConst {
                                    token: EExprToken::ExStringConst,
                                    value: "/Game/_AssemblyStorm/ModIntegration/MI_SpawnMods.MI_SpawnMods_C".to_string()
                                }.into()
                            )
                        }
                        .into()
                    ]
                }
                .into(),
            ),
        }
        .into(),
    );
    inst.insert(
        1,
        ExLet {
            token: EExprToken::ExLet,
            value: KismetPropertyPointer {
                old: None,
                new: Some(FieldPath {
                    path: vec![prop_transform_name.clone()],
                    resolved_owner: fi,
                }),
            },
            variable: Box::new(
                ExLocalVariable {
                    token: EExprToken::ExLocalVariable,
                    variable: KismetPropertyPointer {
                        old: None,
                        new: Some(FieldPath {
                            path: vec![prop_transform_name.clone()],
                            resolved_owner: fi,
                        }),
                    },
                }
                .into(),
            ),
            expression: Box::new(ex_transform.into()),
        }
        .into(),
    );

    inst.insert(
        2,
        ExLetObj {
            token: EExprToken::ExLetObj,
            variable_expression: Box::new(
                ExLocalVariable {
                    token: EExprToken::ExLocalVariable,
                    variable: KismetPropertyPointer {
                        old: None,
                        new: Some(FieldPath {
                            path: vec![prop_begin_spawn_name.clone()],
                            resolved_owner: fi,
                        }),
                    },
                }
                .into(),
            ),
            assignment_expression: Box::new(
                ExCallMath {
                    token: EExprToken::ExCallMath,
                    stack_node: begin_spawning,
                    parameters: vec![
                        ExSelf {
                            token: EExprToken::ExSelf,
                        }
                        .into(),
                        ExLocalVariable {
                            token: EExprToken::ExLocalVariable,
                            variable: KismetPropertyPointer {
                                old: None,
                                new: Some(FieldPath {
                                    path: vec![prop_class_name],
                                    resolved_owner: fi,
                                }),
                            },
                        }
                        .into(),
                        ExLocalVariable {
                            token: EExprToken::ExLocalVariable,
                            variable: KismetPropertyPointer {
                                old: None,
                                new: Some(FieldPath {
                                    path: vec![prop_transform_name.clone()],
                                    resolved_owner: fi,
                                }),
                            },
                        }
                        .into(),
                        ExByteConst {
                            token: EExprToken::ExByteConst,
                            value: 1,
                        }
                        .into(),
                        ExSelf {
                            token: EExprToken::ExSelf,
                        }
                        .into(),
                    ],
                }
                .into(),
            ),
        }
        .into(),
    );

    inst.insert(
        3,
        ExCallMath {
            token: EExprToken::ExCallMath,
            stack_node: finish_spawning,
            parameters: vec![
                ExLocalVariable {
                    token: EExprToken::ExLocalVariable,
                    variable: KismetPropertyPointer {
                        old: None,
                        new: Some(FieldPath {
                            path: vec![prop_begin_spawn_name],
                            resolved_owner: fi,
                        }),
                    },
                }
                .into(),
                ExLocalVariable {
                    token: EExprToken::ExLocalVariable,
                    variable: KismetPropertyPointer {
                        old: None,
                        new: Some(FieldPath {
                            path: vec![prop_transform_name],
                            resolved_owner: fi,
                        }),
                    },
                }
                .into(),
            ],
        }
        .into(),
    );
}

fn inject_init_actors<R: Read + Seek>(
    asset: &mut Asset<R>,
    init_spacerig: HashSet<String>,
    init_cave: HashSet<String>,
    mods: &[String],
) {
    let init_spacerig_fnames = init_spacerig
        .into_iter()
        .map(|p| asset.add_fname(&p))
        .collect::<Vec<_>>();
    let init_cave_fnames = init_cave
        .into_iter()
        .map(|p| asset.add_fname(&p))
        .collect::<Vec<_>>();

    let ancestry = Ancestry::new(FName::new_dummy("".to_owned(), 0));

    let structs = mods
        .iter()
        .map(|m| {
            StructProperty {
                name: asset.add_fname("LoadedMods"),
                ancestry: Ancestry::new(FName::new_dummy("".to_owned(), 0)),
                struct_type: Some(asset.add_fname("MI_Mod")),
                struct_guid: Some([
                    59, 201, 35, 171, 89, 71, 206, 180, 185, 207, 203, 190, 80, 216, 194, 203,
                ]),
                property_guid: None,
                duplication_index: 0,
                serialize_none: true,
                value: [
                    StrProperty {
                        name: asset.add_fname("Name_2_34C619CC6D494CA58075DEC3D6BA8888"),
                        ancestry: ancestry.clone(),
                        property_guid: None,
                        duplication_index: 0,
                        value: Some(m.to_owned()),
                    }
                    .into(),
                    StrProperty {
                        name: asset.add_fname("ID_6_9947C5279BF5459380939CBA188C9805"),
                        ancestry: ancestry.clone(),
                        property_guid: None,
                        duplication_index: 0,
                        value: Some("".to_string()),
                    }
                    .into(),
                    StrProperty {
                        name: asset.add_fname("Version_7_B0FB8B97A09949F59B8F7142D9DA23A4"),
                        ancestry: ancestry.clone(),
                        property_guid: None,
                        duplication_index: 0,
                        value: Some("".to_string()),
                    }
                    .into(),
                    BoolProperty {
                        name: asset.add_fname("Required_9_B258E5345EEE4548A6292DEF342D3FBB"),
                        ancestry: ancestry.clone(),
                        property_guid: None,
                        duplication_index: 0,
                        value: false,
                    }
                    .into(),
                ]
                .to_vec(),
            }
            .into()
        })
        .collect::<Vec<Property>>();

    if let Some(e) = find_export_named(asset, "Default__MI_SpawnMods_C") {
        if let Some((_, p)) = find_array_property_named(e, "SpaceRigMods") {
            p.value.clear();
            for path in init_spacerig_fnames {
                p.value.push(
                    SoftObjectProperty {
                        name: FName::new_dummy("0".to_owned(), -2147483648),
                        ancestry: ancestry.clone(),
                        property_guid: None,
                        duplication_index: 0,
                        value: SoftObjectPath {
                            asset_path: TopLevelAssetPath::new(None, path),
                            sub_path_string: None,
                        },
                    }
                    .into(),
                );
            }
        }
        if let Some((_, p)) = find_array_property_named(e, "CaveMods") {
            p.value.clear();
            for path in init_cave_fnames {
                p.value.push(
                    SoftObjectProperty {
                        name: FName::new_dummy("0".to_owned(), -2147483648),
                        ancestry: ancestry.clone(),
                        property_guid: None,
                        duplication_index: 0,
                        value: SoftObjectPath {
                            asset_path: TopLevelAssetPath::new(None, path),
                            sub_path_string: None,
                        },
                    }
                    .into(),
                );
            }
        }

        if let Some((i, p)) = find_array_property_named(e, "LoadedMods") {
            if structs.is_empty() {
                e.properties.remove(i);
            } else {
                p.value = structs;
            }
        }
    }
}

fn patch<C: Seek + Read>(asset: &mut Asset<C>) -> Result<()> {
    let ver = splice::AssetVersion::new_from(asset);
    let mut statements = splice::extract_tracked_statements(asset, ver, &None);

    let find_function = |name| {
        asset
            .imports
            .iter()
            .enumerate()
            .find(|(_, i)| {
                i.class_package.get_content(|s| s == "/Script/CoreUObject")
                    && i.class_name.get_content(|s| s == "Function")
                    && i.object_name.get_content(|s| s == name)
            })
            .map(|(pi, _)| PackageIndex::from_import(pi as i32).unwrap())
    };

    fn patch_ismodded(
        is_modded: Option<PackageIndex>,
        is_modded_sandbox: Option<PackageIndex>,
        mut statement: TrackedStatement,
    ) -> Option<TrackedStatement> {
        splice::walk(&mut statement.ex, &|ex| {
            if let KismetExpression::ExCallMath(f) = ex {
                if Some(f.stack_node) == is_modded || Some(f.stack_node) == is_modded_sandbox {
                    *ex = ExFalse::default().into()
                }
            }
        });
        Some(statement)
    }

    let is_modded = find_function("FSDIsModdedServer");
    let is_modded_sandbox = find_function("FSDIsModdedSandboxServer");

    for (_pi, statements) in statements.iter_mut() {
        *statements = std::mem::take(statements)
            .into_iter()
            .filter_map(|s| patch_ismodded(is_modded, is_modded_sandbox, s))
            .collect();
    }
    splice::inject_tracked_statements(asset, ver, statements);
    Ok(())
}
