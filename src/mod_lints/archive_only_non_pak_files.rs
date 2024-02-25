use std::collections::BTreeSet;

use crate::providers::ModSpecification;

use super::{Lint, LintCtxt, LintError};

#[derive(Default)]
pub struct ArchiveOnlyNonPakFilesLint;

impl Lint for ArchiveOnlyNonPakFilesLint {
    type Output = BTreeSet<ModSpecification>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output, LintError> {
        let mut archive_only_non_pak_files_mods = BTreeSet::new();
        lcx.for_each_mod(
            |_, _, _| Ok(()),
            None::<fn(ModSpecification)>,
            Some(|mod_spec| {
                archive_only_non_pak_files_mods.insert(mod_spec);
            }),
            None::<fn(ModSpecification)>,
        )?;
        Ok(archive_only_non_pak_files_mods)
    }
}
