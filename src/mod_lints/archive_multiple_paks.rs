use std::collections::BTreeSet;

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt, LintError};

#[derive(Default)]
pub struct ArchiveMultiplePaksLint;

impl Lint for ArchiveMultiplePaksLint {
    type Output = BTreeSet<ModSpecification>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output, LintError> {
        let mut archive_multiple_paks_mods = BTreeSet::new();
        lcx.for_each_mod(
            |_, _, _| Ok(()),
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
            Some(|mod_spec| {
                archive_multiple_paks_mods.insert(mod_spec);
            }),
        )?;
        Ok(archive_multiple_paks_mods)
    }
}
