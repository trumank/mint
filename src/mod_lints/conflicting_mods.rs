use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt};

#[derive(Default)]
pub struct ConflictingModsLint;

const CONFLICTING_MODS_LINT_WHITELIST: [&str; 1] = ["fsd/content/_interop"];

impl Lint for ConflictingModsLint {
    type Output = BTreeMap<String, BTreeSet<ModSpecification>>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output> {
        let mut per_path_modifiers = BTreeMap::new();

        lcx.for_each_mod_file(
            |mod_spec, _, _, normalized_path| {
                per_path_modifiers
                    .entry(normalized_path)
                    .and_modify(|modifiers: &mut BTreeSet<ModSpecification>| {
                        modifiers.insert(mod_spec.clone());
                    })
                    .or_insert_with(|| [mod_spec.clone()].into());
                Ok(())
            },
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
        )?;

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
            .filter(|(_, modifiers)| modifiers.len() > 1)
            .collect::<BTreeMap<String, BTreeSet<ModSpecification>>>();

        Ok(conflicting_mods)
    }
}
