use std::collections::BTreeMap;

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt, LintError};

#[derive(Default)]
pub struct OutdatedPakVersionLint;

impl Lint for OutdatedPakVersionLint {
    type Output = BTreeMap<ModSpecification, repak::Version>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output, LintError> {
        let mut outdated_pak_version_mods = BTreeMap::new();

        lcx.for_each_mod(
            |mod_spec, _, pak_reader| {
                if pak_reader.version() < repak::Version::V11 {
                    outdated_pak_version_mods.insert(mod_spec.clone(), pak_reader.version());
                }
                Ok(())
            },
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
        )?;

        Ok(outdated_pak_version_mods)
    }
}
