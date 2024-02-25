use std::collections::{BTreeMap, BTreeSet};

use tracing::trace;

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt, LintError};

#[derive(Default)]
pub struct SplitAssetPairsLint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SplitAssetPair {
    MissingUexp,
    MissingUasset,
}

impl Lint for SplitAssetPairsLint {
    type Output = BTreeMap<ModSpecification, BTreeMap<String, SplitAssetPair>>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output, LintError> {
        let mut per_mod_path_without_final_ext_to_exts_map = BTreeMap::new();

        lcx.for_each_mod_file(|mod_spec, _, _, _, normalized_path| {
            let mut iter = normalized_path.rsplit('.').take(2);
            let Some(final_ext) = iter.next() else {
                return Ok(());
            };
            let Some(path_without_final_ext) = iter.next() else {
                return Ok(());
            };

            per_mod_path_without_final_ext_to_exts_map
                .entry(mod_spec)
                .and_modify(|map: &mut BTreeMap<String, BTreeSet<String>>| {
                    map.entry(path_without_final_ext.to_string())
                        .and_modify(|exts: &mut BTreeSet<String>| {
                            exts.insert(final_ext.to_string());
                        })
                        .or_insert_with(|| [final_ext.to_string()].into());
                })
                .or_insert_with(|| {
                    [(
                        path_without_final_ext.to_string(),
                        [final_ext.to_string()].into(),
                    )]
                    .into()
                });

            Ok(())
        })?;

        let mut split_asset_pairs_mods = BTreeMap::new();

        for (mod_spec, map) in per_mod_path_without_final_ext_to_exts_map {
            for (path_without_final_ext, final_exts) in map {
                split_asset_pairs_mods
                    .entry(mod_spec.clone())
                    .and_modify(|map: &mut BTreeMap<String, SplitAssetPair>| {
                        match (final_exts.contains("uexp"), final_exts.contains("uasset")) {
                            (true, false) => {
                                map.insert(
                                    format!("{path_without_final_ext}.uexp"),
                                    SplitAssetPair::MissingUasset,
                                );
                            }
                            (false, true) => {
                                map.insert(
                                    format!("{path_without_final_ext}.uasset"),
                                    SplitAssetPair::MissingUexp,
                                );
                            }
                            _ => {}
                        }
                    })
                    .or_insert_with(|| {
                        match (final_exts.contains("uexp"), final_exts.contains("uasset")) {
                            (true, false) => [(
                                format!("{path_without_final_ext}.uexp"),
                                SplitAssetPair::MissingUasset,
                            )]
                            .into(),
                            (false, true) => [(
                                format!("{path_without_final_ext}.uasset"),
                                SplitAssetPair::MissingUexp,
                            )]
                            .into(),
                            _ => BTreeMap::default(),
                        }
                    });
            }
        }

        split_asset_pairs_mods.retain(|_, map| !map.is_empty());

        trace!("split_asset_pairs_mods:\n{:#?}", split_asset_pairs_mods);

        Ok(split_asset_pairs_mods)
    }
}
