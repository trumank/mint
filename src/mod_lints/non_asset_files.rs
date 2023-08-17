use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt};

#[derive(Default)]
pub struct NonAssetFilesLint;

const ENDS_WITH_WHITE_LIST: [&str; 7] = [
    ".uexp",
    ".uasset",
    ".ubulk",
    ".ufont",
    ".locres",
    ".ushaderbytecode",
    "assetregistry.bin",
];

impl Lint for NonAssetFilesLint {
    type Output = BTreeMap<ModSpecification, BTreeSet<String>>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output> {
        let mut non_asset_files = BTreeMap::new();

        lcx.for_each_mod_file(|mod_spec, _, _, _, normalized_path| {
            let is_unreal_asset = ENDS_WITH_WHITE_LIST
                .iter()
                .any(|end| normalized_path.ends_with(end));
            if !is_unreal_asset {
                non_asset_files
                    .entry(mod_spec)
                    .and_modify(|files: &mut BTreeSet<String>| {
                        files.insert(normalized_path.clone());
                    })
                    .or_insert_with(|| [normalized_path].into());
            }
            Ok(())
        })?;

        Ok(non_asset_files)
    }
}
