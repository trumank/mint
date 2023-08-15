use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use tracing::{info, span, trace, Level};

use crate::providers::ModSpecification;
use crate::{lint_get_all_files_from_data, open_file, GetAllFilesFromDataError, PakOrNotPak};

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SplitUasset {
    MissingUasset,
    MissingUexp,
}

#[derive(Debug, Clone)]
pub struct ModLintReport {
    pub conflicting_mods: BTreeMap<String, BTreeSet<(ModSpecification, Vec<u8>)>>,
    pub asset_register_bin_mods: BTreeMap<ModSpecification, BTreeSet<String>>,
    pub shader_file_mods: BTreeMap<ModSpecification, BTreeSet<String>>,
    pub outdated_pak_version_mods: BTreeMap<ModSpecification, repak::Version>,
    pub empty_archive_mods: BTreeSet<ModSpecification>,
    pub archive_with_only_non_pak_files_mods: BTreeSet<ModSpecification>,
    pub archive_with_multiple_paks_mods: BTreeSet<ModSpecification>,
    pub non_asset_file_mods: BTreeMap<ModSpecification, BTreeSet<String>>,
    pub split_uasset_uexp_mods: BTreeMap<ModSpecification, BTreeMap<String, SplitUasset>>,
    pub unmodified_base_game_assets: BTreeMap<ModSpecification, BTreeSet<String>>,
}

pub fn lint<P: AsRef<Path>>(
    reference_pak_path: P,
    mods: &[(ModSpecification, PathBuf)],
) -> Result<ModLintReport> {
    trace!(?mods);
    let span = span!(Level::TRACE, "mod_lint");
    let _enter = span.enter();

    info!(target: "mod_lint", "beginning mods lint");

    let mut base_game_asset_hashes: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut per_path_modifiers = BTreeMap::new();
    let mut asset_register_bin_mods = BTreeMap::new();
    let mut shader_file_mods = BTreeMap::new();
    let mut outdated_pak_version_mods = BTreeMap::new();
    let mut archive_with_only_non_pak_files_mods = BTreeSet::new();
    let mut empty_archive_mods = BTreeSet::new();
    let mut archive_with_multiple_paks_mods = BTreeSet::new();
    let mut non_asset_file_mods = BTreeMap::new();
    let mut path_extensions_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut split_uasset_uexp_mods = BTreeMap::new();
    let mut unmodified_base_game_assets = BTreeMap::new();

    let mut reference_pak_buf = match lint_get_all_files_from_data(Box::new(BufReader::new(
        open_file(reference_pak_path)?,
    ))) {
        Ok(mut bufs) => {
            if bufs.len() != 1 {
                bail!("reference pak path is an unexpected archive containing more than one file");
            }

            let (_, pak_or_non_pak) = bufs.remove(0);
            match pak_or_non_pak {
                PakOrNotPak::Pak(pak) => pak,
                PakOrNotPak::NotPak(_) => {
                    bail!("first file in reference pak acrhive contains a non-pak")
                }
            }
        }
        Err(e) => match e {
            GetAllFilesFromDataError::EmptyArchive | GetAllFilesFromDataError::OnlyNonPakFiles => {
                bail!("invalid reference pak");
            }
            GetAllFilesFromDataError::Other(e) => return Err(e),
        },
    };

    let reference_pak_reader = repak::PakReader::new_any(&mut reference_pak_buf, None)?;
    for p in reference_pak_reader.files() {
        if !p.starts_with("FSD") {
            continue;
        }

        trace!("hashing reference pak path: `{}`", p);
        let mount = Path::new(reference_pak_reader.mount_point());
        let j = mount.join(&p);
        let new_path = j
            .strip_prefix("../../../")
            .context("prefix does not match")?;
        let new_path_str = &new_path.to_string_lossy().replace('\\', "/");
        let lowercase = new_path_str.to_ascii_lowercase();

        let mut buf = vec![];
        let mut writer = Cursor::new(&mut buf);
        reference_pak_reader.read_file(&p, &mut reference_pak_buf, &mut writer)?;
        let mut hasher = Sha256::new();
        hasher.update(&buf);
        let hash = hasher.finalize().to_vec();

        base_game_asset_hashes.insert(lowercase, hash);
    }

    for (mod_spec, mod_pak_path) in mods {
        trace!(?mod_spec, ?mod_pak_path);

        let bufs = match lint_get_all_files_from_data(Box::new(BufReader::new(open_file(
            mod_pak_path,
        )?))) {
            Ok(bufs) => bufs,
            Err(e) => match e {
                GetAllFilesFromDataError::EmptyArchive => {
                    empty_archive_mods.insert(mod_spec.clone());
                    continue;
                }
                GetAllFilesFromDataError::OnlyNonPakFiles => {
                    archive_with_only_non_pak_files_mods.insert(mod_spec.clone());
                    continue;
                }
                GetAllFilesFromDataError::Other(e) => return Err(e),
            },
        };

        let mut pak_bufs = bufs
            .into_iter()
            .filter_map(|(p, pak_or_non_pak)| match pak_or_non_pak {
                PakOrNotPak::Pak(pak_buf) => Some((p, pak_buf)),
                PakOrNotPak::NotPak(_) => None,
            })
            .collect::<Vec<_>>();

        if pak_bufs.len() > 1 {
            archive_with_multiple_paks_mods.insert(mod_spec.clone());
        }

        let pak = repak::PakReader::new_any(&mut pak_bufs[0].1, None)?;

        if pak.version() < repak::Version::V11 {
            outdated_pak_version_mods.insert(mod_spec.clone(), pak.version());
        }

        let mount = Path::new(pak.mount_point());

        for p in pak.files() {
            trace!(?p);
            let j = mount.join(&p);
            let new_path = j
                .strip_prefix("../../../")
                .context("prefix does not match")?;
            let new_path_str = &new_path.to_string_lossy().replace('\\', "/");
            let lowercase = new_path_str.to_ascii_lowercase();

            // Note that including ushaderbytecode is handled by the specific shader-inclusion lint,
            // so we avoid duplicating diagnostics here.
            if !(lowercase.ends_with(".uexp")
                || lowercase.ends_with(".uasset")
                || lowercase.ends_with(".ubulk")
                || lowercase.ends_with(".ufont")
                || lowercase.ends_with("assetregistry.bin")
                || lowercase.ends_with(".ushaderbytecode"))
            {
                trace!("file is not known unreal asset: `{}`", lowercase);
                non_asset_file_mods
                    .entry(mod_spec.clone())
                    .and_modify(|files: &mut BTreeSet<String>| {
                        files.insert(lowercase.clone());
                    })
                    .or_insert_with(|| [lowercase.clone()].into());
            }

            let path_without_ext = if let Some(path_without_ext) = p.rsplit('.').nth(1) {
                path_without_ext.to_string()
            } else {
                p.clone()
            };

            path_extensions_map
                .entry(path_without_ext.clone())
                .and_modify(|extensions| {
                    if let Some(ext) = p.rsplit('.').next() {
                        extensions.insert(ext.to_string());
                    }
                })
                .or_insert_with(|| {
                    if let Some(ext) = p.rsplit('.').next() {
                        [ext.to_string()].into()
                    } else {
                        BTreeSet::default()
                    }
                });

            let mut buf = vec![];
            let mut writer = Cursor::new(&mut buf);
            pak.read_file(&p, &mut pak_bufs[0].1, &mut writer)?;
            let mut hasher = Sha256::new();
            trace!("buf.len() = {}", buf.len());
            hasher.update(&buf);
            let hash = hasher.finalize().to_vec();
            trace!("hash = {}", hex::encode(&hash));

            per_path_modifiers
                .entry(lowercase.clone())
                .and_modify(|modifiers: &mut BTreeSet<(ModSpecification, Vec<u8>)>| {
                    modifiers.insert((mod_spec.clone(), hash.clone()));
                })
                .or_insert_with(|| [(mod_spec.clone(), hash.clone())].into());

            if let Some(base_game_asset_hash) = base_game_asset_hashes.get(&lowercase)
                && base_game_asset_hash == &hash
            {
                unmodified_base_game_assets.entry(mod_spec.clone())
                    .and_modify(|files: &mut BTreeSet<String>| {
                        files.insert(lowercase.clone());
                    })
                    .or_insert_with(|| [lowercase.clone()].into());
            }

            if let Some(filename) = new_path.file_name() {
                if filename == "AssetRegistry.bin" {
                    asset_register_bin_mods
                        .entry(mod_spec.clone())
                        .and_modify(|paths: &mut BTreeSet<String>| {
                            paths.insert(lowercase.clone());
                        })
                        .or_insert_with(|| [lowercase.clone()].into());
                }
                if new_path.extension().and_then(std::ffi::OsStr::to_str) == Some("ushaderbytecode")
                {
                    shader_file_mods
                        .entry(mod_spec.clone())
                        .and_modify(|paths: &mut BTreeSet<String>| {
                            paths.insert(lowercase.clone());
                        })
                        .or_insert_with(|| [lowercase].into());
                }
            }
        }

        path_extensions_map
            .iter()
            .for_each(|(path_without_ext, exts)| {
                match (exts.contains("uasset"), exts.contains("uexp")) {
                    (true, false) => {
                        split_uasset_uexp_mods
                            .entry(mod_spec.clone())
                            .and_modify(
                                |mismatched_pairs_map: &mut BTreeMap<String, SplitUasset>| {
                                    mismatched_pairs_map.insert(
                                        format!("{path_without_ext}.uasset"),
                                        SplitUasset::MissingUexp,
                                    );
                                },
                            )
                            .or_insert_with(|| {
                                [(
                                    format!("{path_without_ext}.uasset"),
                                    SplitUasset::MissingUexp,
                                )]
                                .into()
                            });
                    }
                    (false, true) => {
                        split_uasset_uexp_mods
                            .entry(mod_spec.clone())
                            .and_modify(
                                |mismatched_pairs_map: &mut BTreeMap<String, SplitUasset>| {
                                    mismatched_pairs_map.insert(
                                        format!("{path_without_ext}.uexp"),
                                        SplitUasset::MissingUasset,
                                    );
                                },
                            )
                            .or_insert_with(|| {
                                [(
                                    format!("{path_without_ext}.uexp"),
                                    SplitUasset::MissingUasset,
                                )]
                                .into()
                            });
                    }
                    _ => {}
                }
            });
    }

    const CONFLICTING_MODS_LINT_WHITELIST: [&str; 1] = ["fsd/content/_interop"];

    let conflicting_mods = per_path_modifiers
        .into_iter()
        .filter(|(p, _)| {
            for whitelisted_path in CONFLICTING_MODS_LINT_WHITELIST {
                if p.starts_with(whitelisted_path) {
                    return false;
                }
            }
            true
        })
        .filter(|(_, modifiers)| {
            modifiers.len() > 1 && {
                let mut first_hash = None;
                for (_, hash) in modifiers {
                    match first_hash {
                        None => {
                            first_hash = Some(hash);
                        }
                        Some(first_hash) => {
                            if hash != first_hash {
                                return true;
                            }
                        }
                    }
                }
                false
            }
        })
        .collect::<BTreeMap<String, BTreeSet<(ModSpecification, Vec<u8>)>>>();

    Ok(ModLintReport {
        conflicting_mods,
        asset_register_bin_mods,
        shader_file_mods,
        outdated_pak_version_mods,
        empty_archive_mods,
        archive_with_only_non_pak_files_mods,
        archive_with_multiple_paks_mods,
        non_asset_file_mods,
        split_uasset_uexp_mods,
        unmodified_base_game_assets,
    })
}
