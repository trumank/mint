use crate::{get_pak_from_file, populate_config, Config, Mods, STATIC_SETTINGS};

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use modio::download::DownloadAction;

/// Take config, validate against mod.io, install, return populated config
pub async fn install_config(config: &mut Config, mods: Mods, update: bool) -> Result<Mods> {
    println!("installing config={mods:#?}");

    let mut mod_hashes = HashMap::new();
    let mod_config = populate_config(config, mods, update, &mut mod_hashes).await?;

    let mut paks = vec![];

    fs::create_dir(&STATIC_SETTINGS.mod_cache_dir).ok();

    for entry in &mod_config.mods {
        let mod_id = entry.id.parse::<u32>()?;
        if let Some(version) = &entry.version {
            let file_id = version.parse::<u32>()?;
            let file_path = &STATIC_SETTINGS.mod_cache_dir.join(format!("{file_id}.zip"));
            if !file_path.exists() {
                println!(
                    "downloading mod={} version={} path={}",
                    mod_id,
                    file_id,
                    file_path.display()
                );
                config
                    .settings
                    .modio()
                    .expect("could not create modio object")
                    .download(DownloadAction::File {
                        game_id: STATIC_SETTINGS.game_id,
                        mod_id,
                        file_id,
                    })
                    .save_to_file(&file_path)
                    .await?;
            }

            let modfile;
            let hash = if let Some(hash) = mod_hashes.get(&file_id) {
                hash
            } else {
                println!("requesting modfile={file_id}");
                modfile = config
                    .settings
                    .modio()
                    .expect("could not create modio object")
                    .game(STATIC_SETTINGS.game_id)
                    .mod_(mod_id)
                    .file(file_id)
                    .get()
                    .await?;
                &modfile.filehash.md5
            };

            use md5::{Digest, Md5};
            let mut hasher = Md5::new();
            std::io::copy(&mut File::open(file_path)?, &mut hasher)?;
            let local_hash = hex::encode(hasher.finalize());
            println!("checking file hash modio={hash} local={local_hash}");
            assert_eq!(hash, &local_hash);

            let buf = get_pak_from_file(file_path)?;
            paks.push((format!("{mod_id}"), buf));
        } else {
            panic!("unreachable");
        }
    }

    let mod_count = paks.len();
    let loader = include_bytes!("../mod-integration.pak").to_vec();
    paks.push(("loader".to_string(), loader));

    /*
    let mut fsd_pak = repak::PakReader::new_any(
        BufReader::new(File::open(
            config
                .settings
                .paks_dir()
                .expect("could not find paks directory")
                .join("FSD-WindowsNoEditor.pak"),
        )?),
        None,
    )?;
    let bytes_asset = fsd_pak.get("FSD/Content/Game/BP_PlayerControllerBase.uasset")?;
    let bytes_exp = fsd_pak.get("FSD/Content/Game/BP_PlayerControllerBase.uexp")?;
    let mut asset = unreal_asset::Asset::new(bytes_asset, Some(bytes_exp));
    asset.set_engine_version(unreal_asset::engine_version::EngineVersion::VER_UE4_27);
    asset.parse_data().unwrap();
    for export in asset.exports {
        if let unreal_asset::exports::Export::FunctionExport(func) = export {
            if func.struct_export.normal_export.base_export.object_name.content == "ReceiveBeginPlay" {
                println!("{:#?}", func);
            }
        }
    }
    */

    /*
    // no longer necessary since only a single file is changed by the integrator
    // TODO: perhaps warn the user if any other paks are present which may conflict/crash?
    for entry in fs::read_dir(
        config
            .settings
            .paks_dir()
            .expect("could not find paks directory"),
    )
    .expect("Unable to list")
    {
        let entry = entry.expect("unable to get entry");
        if entry.file_type()?.is_dir() {
            continue;
        };
        if let Some(name) = entry.file_name().to_str() {
            if name.ends_with(".pak") && name != "FSD-WindowsNoEditor.pak" {
                fs::remove_file(entry.path())?;
            }
        }
    }
    */

    let out_file = std::io::BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(
                config
                    .settings
                    .paks_dir()
                    .expect("could not find paks dir")
                    .join("mods_P.pak"),
            )?,
    );

    let mount_point = PathBuf::from("../../../");
    let mut out_pak = repak::PakWriter::new(
        out_file,
        None,
        repak::Version::V8B,
        String::from(mount_point.to_string_lossy()),
    );

    let integrator_path_asset =
        Path::new("../../../FSD/Content/_AssemblyStorm/ModIntegration/MI_SpawnMods.uasset");
    let integrator_path_exp =
        Path::new("../../../FSD/Content/_AssemblyStorm/ModIntegration/MI_SpawnMods.uexp");
    let mut integrator_asset = None;
    let mut integrator_exp = None;

    let mut init_spacerig_assets = HashSet::new();
    let mut init_cave_assets = HashSet::new();
    let mut asset_mod_owner = HashMap::new();
    for (_id, buf) in paks {
        let mut in_pak = repak::PakReader::new_any(std::io::Cursor::new(&buf), None)?;
        let in_mount_point = PathBuf::from(in_pak.mount_point());
        for file in in_pak.files() {
            let path = in_mount_point.join(&file);
            let filename = path
                .file_name()
                .ok_or(anyhow!("failed to get asset name: {}", path.display()))?
                .to_string_lossy();
            if filename == "AssetRegistry.bin" {
                continue;
            }
            fn format_soft_class(path: &Path) -> String {
                let name = path.file_stem().unwrap().to_string_lossy();
                format!(
                    "/Game/{}{}_C",
                    path.strip_prefix("../../../FSD/Content")
                        .unwrap()
                        .to_string_lossy()
                        .strip_suffix("uasset")
                        .unwrap(),
                    name
                )
            }
            if filename.to_lowercase() == "initspacerig.uasset" {
                init_spacerig_assets.insert(format_soft_class(&path));
            }
            if filename.to_lowercase() == "initcave.uasset" {
                init_cave_assets.insert(format_soft_class(&path));
            }
            let data = in_pak.get(&file)?;
            if path == integrator_path_asset {
                integrator_asset = Some(data);
            } else if path == integrator_path_exp {
                integrator_exp = Some(data);
            } else {
                let out_path = String::from(path.strip_prefix(&mount_point)?.to_string_lossy());

                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&data);
                let hash = hasher.finalize();

                if let Some((owner, existing_hash)) = asset_mod_owner.get(&out_path) {
                    if hash != *existing_hash {
                        println!(
                            "warn: {} overwrote asset added by {}: {}",
                            owner, _id, out_path
                        );
                    }
                } else {
                    out_pak.write_file(&out_path, &mut std::io::Cursor::new(data))?;
                }
                asset_mod_owner.insert(out_path.clone(), (_id.to_owned(), hash));
            }
        }
    }

    println!("found InitSpaceRig assets {:#?}", &init_spacerig_assets);
    println!("found InitCave assets {:#?}", &init_cave_assets);

    let mut asset =
        unreal_asset::Asset::new(integrator_asset.unwrap(), Some(integrator_exp.unwrap()));
    asset.set_engine_version(unreal_asset::engine_version::EngineVersion::VER_UE4_27);
    asset.parse_data().unwrap();
    let init_spacerig_fnames = init_spacerig_assets
        .into_iter()
        .map(|p| asset.add_fname(&p))
        .collect::<Vec<_>>();
    let init_cave_fnames = init_cave_assets
        .into_iter()
        .map(|p| asset.add_fname(&p))
        .collect::<Vec<_>>();
    fn find_export_named<'a>(
        asset: &'a mut unreal_asset::Asset,
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
    fn find_string_property_named<'a>(
        export: &'a mut unreal_asset::exports::normal_export::NormalExport,
        name: &'a str,
    ) -> Option<&'a mut unreal_asset::properties::str_property::StrProperty> {
        for prop in &mut export.properties {
            if let unreal_asset::properties::Property::StrProperty(prop) = prop {
                if prop.name.content == name {
                    return Some(prop);
                }
            }
        }
        None
    }
    fn find_array_property_named<'a>(
        export: &'a mut unreal_asset::exports::normal_export::NormalExport,
        name: &'a str,
    ) -> Option<&'a mut unreal_asset::properties::array_property::ArrayProperty> {
        for prop in &mut export.properties {
            if let unreal_asset::properties::Property::ArrayProperty(prop) = prop {
                if prop.name.content == name {
                    return Some(prop);
                }
            }
        }
        None
    }
    let config_str = serde_json::to_string(&mod_config)?;
    if let Some(e) = find_export_named(&mut asset, "Default__MI_SpawnMods_C") {
        if let Some(p) = find_string_property_named(e, "Config") {
            p.value = Some(config_str);
        }
        if let Some(p) = find_array_property_named(e, "SpaceRigMods") {
            for path in init_spacerig_fnames {
                p.value
                    .push(unreal_asset::properties::Property::SoftObjectProperty(
                        unreal_asset::properties::object_property::SoftObjectProperty {
                            name: unreal_asset::types::FName::new("0".to_owned(), -2147483648),
                            property_guid: None,
                            duplication_index: 0,
                            value: unreal_asset::properties::object_property::SoftObjectPath {
                                asset_path_name: path,
                                sub_path_string: None,
                            },
                        },
                    ));
            }
        }
        if let Some(p) = find_array_property_named(e, "CaveMods") {
            for path in init_cave_fnames {
                p.value
                    .push(unreal_asset::properties::Property::SoftObjectProperty(
                        unreal_asset::properties::object_property::SoftObjectProperty {
                            name: unreal_asset::types::FName::new("0".to_owned(), -2147483648),
                            property_guid: None,
                            duplication_index: 0,
                            value: unreal_asset::properties::object_property::SoftObjectPath {
                                asset_path_name: path,
                                sub_path_string: None,
                            },
                        },
                    ));
            }
        }
    }
    let mut out_asset = std::io::Cursor::new(vec![]);
    let mut out_exp = std::io::Cursor::new(vec![]);
    asset
        .write_data(&mut out_asset, Some(&mut out_exp))
        .unwrap();

    out_pak.write_file(
        &String::from(
            integrator_path_asset
                .strip_prefix(&mount_point)?
                .to_string_lossy(),
        ),
        &mut std::io::Cursor::new(out_asset.into_inner()),
    )?;
    out_pak.write_file(
        &String::from(
            integrator_path_exp
                .strip_prefix(&mount_point)?
                .to_string_lossy(),
        ),
        &mut std::io::Cursor::new(out_exp.into_inner()),
    )?;

    out_pak.write_index()?;

    println!("{mod_count} mods installed");

    Ok(mod_config)
}
