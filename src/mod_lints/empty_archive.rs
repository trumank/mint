use std::collections::BTreeSet;

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt, LintError};

#[derive(Default)]
pub struct EmptyArchiveLint;

impl Lint for EmptyArchiveLint {
    type Output = BTreeSet<ModSpecification>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output, LintError> {
        let mut empty_archive_mods = BTreeSet::new();

        lcx.for_each_mod(
            |_, _, _| Ok(()),
            Some(|mod_spec| {
                empty_archive_mods.insert(mod_spec);
            }),
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
        )?;

        Ok(empty_archive_mods)
    }
}
