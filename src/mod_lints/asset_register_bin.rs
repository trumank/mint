use std::collections::{BTreeMap, BTreeSet};

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt, LintError};

#[derive(Default)]
pub struct AssetRegisterBinLint;

impl Lint for AssetRegisterBinLint {
    type Output = BTreeMap<ModSpecification, BTreeSet<String>>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output, LintError> {
        let mut asset_register_bin_mods = BTreeMap::new();

        lcx.for_each_mod_file(|mod_spec, _, _, raw_path, normalized_path| {
            if let Some(filename) = raw_path.file_name()
                && filename == "AssetRegistry.bin" {
                    asset_register_bin_mods
                        .entry(mod_spec.clone())
                        .and_modify(|paths: &mut BTreeSet<String>| {
                            paths.insert(normalized_path.clone());
                        })
                        .or_insert_with(|| [normalized_path.clone()].into());
                }

            Ok(())
        })?;

        Ok(asset_register_bin_mods)
    }
}
