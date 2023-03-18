use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Cursor, Read, Seek};
use std::path::Path;

use anyhow::{anyhow, Result};

use crate::providers::{Mod, ReadSeek, ResolvableStatus};

use unreal_asset::{
    exports::ExportBaseTrait,
    flags::EObjectFlags,
    kismet::{
        EExprToken, ExByteConst, ExCallMath, ExLet, ExLetObj, ExLocalVariable, ExObjectConst,
        ExRotationConst, ExSelf, ExVectorConst, FieldPath, KismetPropertyPointer,
    },
    properties::{
        int_property::BoolProperty,
        object_property::{SoftObjectPath, SoftObjectProperty},
        str_property::StrProperty,
        struct_property::StructProperty,
        Property,
    },
    types::{FName, PackageIndex},
    Asset,
};

pub fn integrate<P: AsRef<Path>>(path_game: P, mods: Vec<Mod>) -> Result<()> {
    let path_paks = Path::join(path_game.as_ref(), "FSD/Content/Paks/");
    let path_pak = Path::join(&path_paks, "FSD-WindowsNoEditor.pak");
    let path_mod_pak = Path::join(&path_paks, "mods_P.pak");

    let mut fsd_pak_reader = BufReader::new(File::open(path_pak)?);
    let fsd_pak = repak::PakReader::new_any(&mut fsd_pak_reader, None)?;

    let pcb_path = (
        "FSD/Content/Game/BP_PlayerControllerBase.uasset",
        "FSD/Content/Game/BP_PlayerControllerBase.uexp",
    );

    let mut pcb_asset = unreal_asset::Asset::new(
        Cursor::new(fsd_pak.get(pcb_path.0, &mut fsd_pak_reader)?),
        Some(Cursor::new(fsd_pak.get(pcb_path.1, &mut fsd_pak_reader)?)),
    );

    pcb_asset.set_engine_version(unreal_asset::engine_version::EngineVersion::VER_UE4_27);
    pcb_asset.parse_data()?;

    hook_pcb(&mut pcb_asset);

    let mut pcb_out = (Cursor::new(vec![]), Cursor::new(vec![]));

    pcb_asset.write_data(&mut pcb_out.0, Some(&mut pcb_out.1))?;
    pcb_out.0.rewind()?;
    pcb_out.1.rewind()?;

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

    mod_pak.write_file(pcb_path.0, &mut pcb_out.0)?;
    mod_pak.write_file(pcb_path.1, &mut pcb_out.1)?;

    let mut init_spacerig_assets = HashSet::new();
    let mut init_cave_assets = HashSet::new();

    let mods = mods
        .into_iter()
        .map(|m| {
            println!("integrating {m:?}");
            let mut buf = get_pak_from_data(Box::new(BufReader::new(File::open(m.path)?)))?;
            let pak = repak::PakReader::new_any(&mut buf, None)?;

            let mount = Path::new(pak.mount_point());

            for p in pak.files() {
                let j = mount.join(&p);
                let new_path = j.strip_prefix("../../../").expect("prefix does not match");

                if let Some(filename) = new_path.file_name() {
                    if filename == "AssetRegistry.bin" {
                        continue;
                    }
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
                    let lower = filename.to_string_lossy().to_lowercase();
                    if lower == "initspacerig.uasset" {
                        init_spacerig_assets.insert(format_soft_class(new_path));
                    }
                    if lower == "initcave.uasset" {
                        init_cave_assets.insert(format_soft_class(new_path));
                    }
                }

                mod_pak.write_file(
                    &new_path.to_string_lossy().replace('\\', "/"),
                    &mut Cursor::new(pak.get(&p, &mut buf)?),
                )?;
            }
            Ok(match m.status {
                ResolvableStatus::Unresolvable { name } => name,
                ResolvableStatus::Resolvable { url } => url,
            })
        })
        .collect::<Result<Vec<String>>>()?;

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
        mod_pak.write_file(
            new_path,
            &mut Cursor::new(int_pak.get(&p, &mut int_pak_reader)?),
        )?;
    }

    let mut int_asset = unreal_asset::Asset::new(
        Cursor::new(int_pak.get(int_path.0, &mut int_pak_reader)?),
        Some(Cursor::new(int_pak.get(int_path.1, &mut int_pak_reader)?)),
    );

    int_asset.set_engine_version(unreal_asset::engine_version::EngineVersion::VER_UE4_27);
    int_asset.parse_data()?;

    inject_init_actors(
        &mut int_asset,
        init_spacerig_assets,
        init_cave_assets,
        &mods,
    );

    let mut int_out = (Cursor::new(vec![]), Cursor::new(vec![]));

    int_asset.write_data(&mut int_out.0, Some(&mut int_out.1))?;
    int_out.0.rewind()?;
    int_out.1.rewind()?;

    mod_pak.write_file(int_path.0, &mut int_out.0)?;
    mod_pak.write_file(int_path.1, &mut int_out.1)?;

    mod_pak.write_index()?;

    println!(
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
            .ok_or_else(|| anyhow!("Zip does not contain pak"))?
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
                ai.class_package.content == i.class_package
                    && ai.class_name.content == i.class_name
                    && ai.object_name.content == i.object_name
                    && ai.outer_index == pi
            })
            .map(|(index, _)| PackageIndex::from_import(index as i32).unwrap());
        pi = ai.unwrap_or_else(|| {
            let new_import = unreal_asset::Import::new(
                asset.add_fname(i.class_package),
                asset.add_fname(i.class_name),
                pi,
                asset.add_fname(i.object_name),
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
    for export in &mut asset.exports {
        if let unreal_asset::exports::Export::NormalExport(export) = export {
            if export.base_export.object_name.content == name {
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
            if prop.name.content == name {
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
    let mi_spawn_mods = get_import(
        asset,
        vec![
            Import::new(
                "/Script/CoreUObject",
                "Package",
                "/Game/_AssemblyStorm/ModIntegration/MI_SpawnMods",
            ),
            Import::new(
                "/Script/Engine",
                "BlueprintGeneratedClass",
                "MI_SpawnMods_C",
            ),
        ],
    );
    let ex_transform = ExCallMath {
        token: EExprToken::ExCallMath,
        stack_node: make_transform,
        parameters: vec![
            ExVectorConst {
                token: EExprToken::ExVectorConst,
                value: unreal_asset::types::vector::Vector::new(
                    0f32.into(),
                    0f32.into(),
                    0f32.into(),
                ),
            }
            .into(),
            ExRotationConst {
                token: EExprToken::ExVectorConst,
                pitch: 0,
                roll: 0,
                yaw: 0,
            }
            .into(),
            ExVectorConst {
                token: EExprToken::ExVectorConst,
                value: unreal_asset::types::vector::Vector::new(
                    1f32.into(),
                    1f32.into(),
                    1f32.into(),
                ),
            }
            .into(),
        ],
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
        .exports
        .iter_mut()
        .enumerate()
        .find_map(|(i, e)| {
            if let unreal_asset::exports::Export::FunctionExport(func) = e {
                if func.get_base_export().object_name.content == "ReceiveBeginPlay" {
                    return Some((PackageIndex::from_export(i as i32).unwrap(), func));
                }
            }
            None
        })
        .unwrap();

    func.struct_export
        .loaded_properties
        .push(prop_transform.into());
    func.struct_export
        .loaded_properties
        .push(prop_begin_spawn.into());
    let inst = func.struct_export.script_bytecode.as_mut().unwrap();
    inst.insert(
        0,
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
        1,
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
                        ExObjectConst {
                            token: EExprToken::ExObjectConst,
                            value: mi_spawn_mods,
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
        2,
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

    let structs = mods
        .iter()
        .map(|m| {
            StructProperty {
                name: asset.add_fname("LoadedMods"),
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
                        property_guid: None,
                        duplication_index: 0,
                        value: Some(m.to_owned()),
                    }
                    .into(),
                    StrProperty {
                        name: asset.add_fname("ID_6_9947C5279BF5459380939CBA188C9805"),
                        property_guid: None,
                        duplication_index: 0,
                        value: Some("".to_string()),
                    }
                    .into(),
                    StrProperty {
                        name: asset.add_fname("Version_7_B0FB8B97A09949F59B8F7142D9DA23A4"),
                        property_guid: None,
                        duplication_index: 0,
                        value: Some("".to_string()),
                    }
                    .into(),
                    BoolProperty {
                        name: asset.add_fname("Required_9_B258E5345EEE4548A6292DEF342D3FBB"),
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
                        name: FName::new("0".to_owned(), -2147483648),
                        property_guid: None,
                        duplication_index: 0,
                        value: SoftObjectPath {
                            asset_path_name: path,
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
                        name: FName::new("0".to_owned(), -2147483648),
                        property_guid: None,
                        duplication_index: 0,
                        value: SoftObjectPath {
                            asset_path_name: path,
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
