mod archive_multiple_paks;
mod archive_only_non_pak_files;
mod asset_register_bin;
mod conflicting_mods;
mod empty_archive;
mod non_asset_files;
mod outdated_pak_version;
mod shader_files;

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::{Context, Result};
use indexmap::IndexSet;
use repak::PakReader;
use tracing::trace;

use crate::mod_lints::conflicting_mods::ConflictingModsLint;
use crate::providers::ModSpecification;
use crate::{lint_get_all_files_from_data, open_file, GetAllFilesFromDataError, PakOrNotPak};

use self::archive_multiple_paks::ArchiveMultiplePaksLint;
use self::archive_only_non_pak_files::ArchiveOnlyNonPakFilesLint;
use self::asset_register_bin::AssetRegisterBinLint;
use self::empty_archive::EmptyArchiveLint;
use self::non_asset_files::NonAssetFiles;
use self::outdated_pak_version::OutdatedPakVersionLint;
use self::shader_files::ShaderFilesLint;

pub struct LintCtxt {
    mods: BTreeSet<(ModSpecification, PathBuf)>,
}

impl LintCtxt {
    pub fn init(mods: BTreeSet<(ModSpecification, PathBuf)>) -> Result<Self> {
        trace!("LintCtxt::init");
        Ok(Self { mods })
    }

    pub fn for_each_mod<F, EmptyArchiveHandler, OnlyNonPakFilesHandler, MultiplePakFilesHandler>(
        &self,
        mut f: F,
        mut empty_archive_handler: Option<EmptyArchiveHandler>,
        mut only_non_pak_files_handler: Option<OnlyNonPakFilesHandler>,
        mut multiple_pak_files_handler: Option<MultiplePakFilesHandler>,
    ) -> Result<()>
    where
        F: FnMut(ModSpecification, &PakReader) -> Result<()>,
        EmptyArchiveHandler: FnMut(ModSpecification),
        OnlyNonPakFilesHandler: FnMut(ModSpecification),
        MultiplePakFilesHandler: FnMut(ModSpecification),
    {
        for (mod_spec, mod_pak_path) in &self.mods {
            let maybe_archive_reader = Box::new(BufReader::new(open_file(mod_pak_path)?));
            let bufs = match lint_get_all_files_from_data(maybe_archive_reader) {
                Ok(bufs) => bufs,
                Err(e) => match e {
                    GetAllFilesFromDataError::EmptyArchive => {
                        if let Some(ref mut handler) = empty_archive_handler {
                            handler(mod_spec.clone());
                        }
                        continue;
                    }
                    GetAllFilesFromDataError::OnlyNonPakFiles => {
                        if let Some(ref mut handler) = only_non_pak_files_handler {
                            handler(mod_spec.clone());
                        }
                        continue;
                    }
                    GetAllFilesFromDataError::Other(e) => return Err(e),
                },
            };

            let mut individual_pak_readers = bufs
                .into_iter()
                .filter_map(|(_, pak_or_non_pak)| match pak_or_non_pak {
                    PakOrNotPak::Pak(individual_pak_reader) => Some(individual_pak_reader),
                    PakOrNotPak::NotPak(_) => None,
                })
                .collect::<Vec<_>>();

            if individual_pak_readers.len() > 1 {
                if let Some(ref mut handler) = multiple_pak_files_handler {
                    handler(mod_spec.clone());
                }
            }

            let mut first_pak_reader = individual_pak_readers.remove(0);
            let pak_reader = repak::PakReader::new_any(&mut first_pak_reader, None)?;
            f(mod_spec.clone(), &pak_reader)?
        }

        Ok(())
    }

    pub fn for_each_mod_file<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(ModSpecification, &PakReader, PathBuf, String) -> Result<()>,
    {
        self.for_each_mod(
            |mod_spec, pak_reader| {
                let mount = PathBuf::from(pak_reader.mount_point());
                for p in pak_reader.files() {
                    let path = mount.join(&p);
                    let path_buf = path
                        .strip_prefix("../../../")
                        .context("prefix does not match")?;
                    let normalized_path = &path_buf.to_string_lossy().replace('\\', "/");
                    let normalized_path = normalized_path.to_ascii_lowercase();
                    f(
                        mod_spec.clone(),
                        pak_reader,
                        path_buf.to_path_buf(),
                        normalized_path,
                    )?
                }

                Ok(())
            },
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
            None::<fn(ModSpecification)>,
        )
    }
}

pub trait Lint {
    type Output;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output>;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LintId {
    name: &'static str,
}

impl LintId {
    pub fn to_name_lower(&self) -> String {
        self.name.to_ascii_lowercase()
    }

    pub const CONFLICTING: Self = LintId {
        name: "conflicting",
    };
    pub const ASSET_REGISTRY_BIN: Self = LintId {
        name: "asset_registry_bin",
    };
    pub const SHADER_FILES: Self = LintId {
        name: "shader_files",
    };
    pub const OUTDATED_PAK_VERSION: Self = LintId {
        name: "outdated_pak_version",
    };
    pub const EMPTY_ARCHIVE: Self = LintId {
        name: "empty_archive",
    };
    pub const ARCHIVE_WITH_ONLY_NON_PAK_FILES: Self = LintId {
        name: "archive_only_non_pak_files",
    };
    pub const ARCHIVE_WITH_MULTIPLE_PAKS: Self = LintId {
        name: "archive_with_multiple_paks",
    };
    pub const NON_ASSET_FILES: Self = LintId {
        name: "non_asset_files",
    };
}

#[derive(Default, Debug)]
pub struct LintReport {
    pub conflicting_mods: Option<BTreeMap<String, IndexSet<ModSpecification>>>,
    pub asset_register_bin_mods: Option<BTreeMap<ModSpecification, BTreeSet<String>>>,
    pub shader_file_mods: Option<BTreeMap<ModSpecification, BTreeSet<String>>>,
    pub outdated_pak_version_mods: Option<BTreeMap<ModSpecification, repak::Version>>,
    pub empty_archive_mods: Option<BTreeSet<ModSpecification>>,
    pub archive_with_only_non_pak_files_mods: Option<BTreeSet<ModSpecification>>,
    pub archive_with_multiple_paks_mods: Option<BTreeSet<ModSpecification>>,
    pub non_asset_file_mods: Option<BTreeMap<ModSpecification, BTreeSet<String>>>,
}

pub fn run_lints(
    enabled_lints: &BTreeSet<LintId>,
    mods: BTreeSet<(ModSpecification, PathBuf)>,
) -> Result<LintReport> {
    let lint_ctxt = LintCtxt::init(mods)?;
    let mut lint_report = LintReport::default();

    for lint_id in enabled_lints {
        match *lint_id {
            LintId::CONFLICTING => {
                let res = ConflictingModsLint.check_mods(&lint_ctxt)?;
                lint_report.conflicting_mods = Some(res);
            }
            LintId::ASSET_REGISTRY_BIN => {
                let res = AssetRegisterBinLint.check_mods(&lint_ctxt)?;
                lint_report.asset_register_bin_mods = Some(res);
            }
            LintId::SHADER_FILES => {
                let res = ShaderFilesLint.check_mods(&lint_ctxt)?;
                lint_report.shader_file_mods = Some(res);
            }
            LintId::OUTDATED_PAK_VERSION => {
                let res = OutdatedPakVersionLint.check_mods(&lint_ctxt)?;
                lint_report.outdated_pak_version_mods = Some(res);
            }
            LintId::EMPTY_ARCHIVE => {
                let res = EmptyArchiveLint.check_mods(&lint_ctxt)?;
                lint_report.empty_archive_mods = Some(res);
            }
            LintId::ARCHIVE_WITH_ONLY_NON_PAK_FILES => {
                let res = ArchiveOnlyNonPakFilesLint.check_mods(&lint_ctxt)?;
                lint_report.archive_with_only_non_pak_files_mods = Some(res);
            }
            LintId::ARCHIVE_WITH_MULTIPLE_PAKS => {
                let res = ArchiveMultiplePaksLint.check_mods(&lint_ctxt)?;
                lint_report.archive_with_multiple_paks_mods = Some(res);
            }
            LintId::NON_ASSET_FILES => {
                let res = NonAssetFiles.check_mods(&lint_ctxt)?;
                lint_report.non_asset_file_mods = Some(res);
            }
            _ => unimplemented!(),
        }
    }

    Ok(lint_report)
}
