use std::collections::BTreeSet;

use anyhow::Result;

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt};

#[derive(Default)]
pub struct EmptyArchiveLint;

impl Lint for EmptyArchiveLint {
    type Output = BTreeSet<ModSpecification>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output> {
        let mut empty_archive_mods = BTreeSet::new();

        lcx.for_each_mod(
            |_, _| Ok(()),
            Some(|mod_spec| {
                empty_archive_mods.insert(mod_spec);
            }),
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
        )?;

        Ok(empty_archive_mods)
    }
}
