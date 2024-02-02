use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::Path;

use anyhow::{bail, Context, Result};
use tracing::debug;

use unreal_asset::exports::ExportBaseTrait;
use unreal_asset::reader::ArchiveTrait;

use crate::open_file;
use crate::providers::ModSpecification;

use super::{Lint, LintCtxt};

#[derive(Default)]
pub struct AutoVerificationLint;

impl Lint for AutoVerificationLint {
    type Output = BTreeMap<ModSpecification, BTreeSet<String>>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output> {
        let Some(game_pak_path) = &lcx.fsd_pak_path else {
            bail!("UnmodifiedGameAssetsLint requires specifying a valid game pak path");
        };

        let mut fsd_pak_file = open_file(game_pak_path)?;
        let fsd_pak = repak::PakBuilder::new().reader(&mut fsd_pak_file)?;
        let mut fsd_pak_reader = BufReader::new(fsd_pak_file);

        let fsd_lowercase_path_map = fsd_pak
            .files()
            .into_iter()
            .map(|p| (p.to_ascii_lowercase(), p))
            .collect::<HashMap<_, _>>();

        let mut res = BTreeMap::new();

        lcx.for_each_mod(
            |mod_spec, mod_pak_seekable, mod_pak_reader| {
                let mod_affecting_res = check_gameplay_affecting(
                    &fsd_lowercase_path_map,
                    &mut fsd_pak_reader,
                    &fsd_pak,
                    mod_pak_seekable,
                    mod_pak_reader,
                )?;
                if let ModGameplayAffectingResult::Yes(paths) = mod_affecting_res {
                    res.insert(mod_spec, paths);
                }
                Ok(())
            },
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
        )?;

        Ok(res)
    }
}

pub enum ModGameplayAffectingResult {
    No,
    Yes(BTreeSet<String>),
}

pub fn check_gameplay_affecting<F, M>(
    fsd_lowercase_path_map: &HashMap<String, String>,
    fsd_pak: &mut F,
    fsd_pak_reader: &repak::PakReader,
    mod_pak: &mut M,
    mod_pak_reader: &repak::PakReader,
) -> Result<ModGameplayAffectingResult>
where
    F: Read + Seek,
    M: Read + Seek,
{
    debug!("check_gameplay_affecting");

    let mount = Path::new(mod_pak_reader.mount_point());

    let whitelist = [
        "SoundWave",
        "SoundCue",
        "SoundClass",
        "SoundMix",
        "MaterialInstanceConstant",
        "Material",
        "SkeletalMesh",
        "StaticMesh",
        "Texture2D",
        "AnimSequence",
        "Skeleton",
        "StringTable",
    ]
    .into_iter()
    .collect::<HashSet<_>>();

    let check_asset = |data: Vec<u8>| -> Result<bool> {
        debug!("check_asset");
        let asset = unreal_asset::AssetBuilder::new(
            Cursor::new(data),
            unreal_asset::engine_version::EngineVersion::VER_UE4_27,
        )
        .skip_data(true)
        .build()?;

        for export in &asset.asset_data.exports {
            let base = export.get_base_export();
            // don't care about exported classes in this case
            if base.outer_index.index == 0
                && base.class_index.is_import()
                && !asset
                    .get_import(base.class_index)
                    .map(|import| import.object_name.get_content(|c| whitelist.contains(c)))
                    .unwrap_or(false)
            {
                // invalid import or import name is not whitelisted, unknown
                return Ok(true);
            };
        }

        Ok(false)
    };

    let mod_lowercase_path_map = mod_pak_reader
        .files()
        .into_iter()
        .map(|p| -> Result<(String, String)> {
            let j = mount.join(&p);
            let new_path = j
                .strip_prefix("../../../")
                .context("prefix does not match")?;
            let new_path_str = &new_path.to_string_lossy().replace('\\', "/");

            Ok((new_path_str.to_ascii_lowercase(), p))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let mut gameplay_affecting_paths = BTreeSet::new();

    for lower in mod_lowercase_path_map.keys() {
        if let Some((base, ext)) = lower.rsplit_once('.') {
            if ["uasset", "uexp", "umap", "ubulk", "ufont"].contains(&ext) {
                let key_uasset = format!("{base}.uasset");
                let key_umap = format!("{base}.umap");
                // check mod pak for uasset or umap
                // if not found, check fsd pak for uasset or umap
                let asset = if let Some(path) = mod_lowercase_path_map.get(&key_uasset) {
                    mod_pak_reader.get(path, mod_pak)?
                } else if let Some(path) = mod_lowercase_path_map.get(&key_umap) {
                    mod_pak_reader.get(path, mod_pak)?
                } else if let Some(path) = fsd_lowercase_path_map.get(&key_uasset) {
                    fsd_pak_reader.get(path, fsd_pak)?
                } else if let Some(path) = fsd_lowercase_path_map.get(&key_umap) {
                    fsd_pak_reader.get(path, fsd_pak)?
                } else {
                    // not found, unknown
                    gameplay_affecting_paths.insert(lower.to_owned());
                    continue;
                };

                let asset_result = check_asset(asset.clone())?;

                if asset_result {
                    gameplay_affecting_paths.insert(lower.to_owned());
                }
            }
        }
    }

    if gameplay_affecting_paths.is_empty() {
        Ok(ModGameplayAffectingResult::No)
    } else {
        Ok(ModGameplayAffectingResult::Yes(gameplay_affecting_paths))
    }
}
