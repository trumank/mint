use std::collections::{BTreeMap, BTreeSet};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{info, span, trace, Level};

use crate::providers::ModSpecification;
use crate::{lint_get_all_files_from_data, open_file, GetAllFilesFromDataError, PakOrNotPak};

#[derive(Debug)]
pub struct ModLintReport {
    pub conflicting_mods: BTreeMap<String, BTreeSet<ModSpecification>>,
    pub asset_register_bin_mods: BTreeMap<ModSpecification, BTreeSet<String>>,
    pub shader_file_mods: BTreeMap<ModSpecification, BTreeSet<String>>,
    pub outdated_pak_version_mods: BTreeMap<ModSpecification, repak::Version>,
    pub empty_archive_mods: BTreeSet<ModSpecification>,
    pub archive_with_only_non_pak_files_mods: BTreeSet<ModSpecification>,
    pub archive_with_multiple_paks_mods: BTreeSet<ModSpecification>,
}

pub fn lint(mods: &[(ModSpecification, PathBuf)]) -> Result<ModLintReport> {
    let span = span!(Level::TRACE, "mod_lint", ?mods);
    let _enter = span.enter();

    info!(target: "mod_lint", "beginning mods lint");

    let mut added_path_modifiers = BTreeMap::new();
    let mut asset_register_bin_mods = BTreeMap::new();
    let mut shader_file_mods = BTreeMap::new();
    let mut outdated_pak_version_mods = BTreeMap::new();
    let mut archive_with_only_non_pak_files_mods = BTreeSet::new();
    let mut empty_archive_mods = BTreeSet::new();
    let mut archive_with_multiple_paks_mods = BTreeSet::new();

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

        let (ref first_pak_path, ref mut first_pak_buf) = pak_bufs[0];
        trace!(?first_pak_path);

        let pak = repak::PakReader::new_any(first_pak_buf, None)?;

        if pak.version() < repak::Version::V11 {
            outdated_pak_version_mods.insert(mod_spec.clone(), pak.version());
        }

        let mount = Path::new(pak.mount_point());

        for p in pak.files() {
            let j = mount.join(&p);
            let new_path = j
                .strip_prefix("../../../")
                .context("prefix does not match")?;
            let new_path_str = &new_path.to_string_lossy().replace('\\', "/");
            let lowercase = new_path_str.to_ascii_lowercase();
            added_path_modifiers
                .entry(lowercase.clone())
                .and_modify(|modifiers: &mut BTreeSet<ModSpecification>| {
                    modifiers.insert(mod_spec.clone());
                })
                .or_insert_with(|| [mod_spec.clone()].into());

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
    }

    let conflicting_mods = added_path_modifiers
        .into_iter()
        .filter(|(_, modifiers)| modifiers.len() > 1)
        .collect::<BTreeMap<String, BTreeSet<ModSpecification>>>();

    Ok(ModLintReport {
        conflicting_mods,
        asset_register_bin_mods,
        shader_file_mods,
        outdated_pak_version_mods,
        empty_archive_mods,
        archive_with_only_non_pak_files_mods,
        archive_with_multiple_paks_mods,
    })
}
