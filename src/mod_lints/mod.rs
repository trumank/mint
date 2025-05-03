mod archive_multiple_paks;
mod archive_only_non_pak_files;
mod asset_register_bin;
mod conflicting_mods;
mod empty_archive;
mod non_asset_files;
mod outdated_pak_version;
mod shader_files;
mod split_asset_pairs;
mod unmodified_game_assets;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};

use fs_err as fs;
use indexmap::IndexSet;
use repak::PakReader;
use snafu::prelude::*;
use tracing::trace;

use self::archive_multiple_paks::ArchiveMultiplePaksLint;
use self::archive_only_non_pak_files::ArchiveOnlyNonPakFilesLint;
use self::asset_register_bin::AssetRegisterBinLint;
use self::empty_archive::EmptyArchiveLint;
use self::non_asset_files::NonAssetFilesLint;
use self::outdated_pak_version::OutdatedPakVersionLint;
use self::shader_files::ShaderFilesLint;
pub use self::split_asset_pairs::SplitAssetPair;
use self::split_asset_pairs::SplitAssetPairsLint;
use self::unmodified_game_assets::UnmodifiedGameAssetsLint;
use crate::mod_lints::conflicting_mods::ConflictingModsLint;
use crate::providers::{ModSpecification, ReadSeek};

#[derive(Debug, Snafu)]
pub enum LintError {
    #[snafu(transparent)]
    RepakError { source: repak::Error },
    #[snafu(transparent)]
    IoError { source: std::io::Error },
    #[snafu(transparent)]
    PrefixMismatch { source: std::path::StripPrefixError },
    #[snafu(display("empty archive"))]
    EmptyArchive,
    #[snafu(display("zip archive error"))]
    ZipArchiveError,
    #[snafu(display("zip only contains non-pak files"))]
    OnlyNonPakFiles,
    #[snafu(display("some lints require specifying a valid game pak path"))]
    InvalidGamePath,
}

pub struct LintCtxt {
    pub(crate) mods: IndexSet<(ModSpecification, PathBuf)>,
    pub(crate) fsd_pak_path: Option<PathBuf>,
}

impl LintCtxt {
    pub fn init(
        mods: IndexSet<(ModSpecification, PathBuf)>,
        fsd_pak_path: Option<PathBuf>,
    ) -> Result<Self, LintError> {
        trace!("LintCtxt::init");
        Ok(Self { mods, fsd_pak_path })
    }

    pub fn for_each_mod<F, EmptyArchiveHandler, OnlyNonPakFilesHandler, MultiplePakFilesHandler>(
        &self,
        mut f: F,
        mut empty_archive_handler: Option<EmptyArchiveHandler>,
        mut only_non_pak_files_handler: Option<OnlyNonPakFilesHandler>,
        mut multiple_pak_files_handler: Option<MultiplePakFilesHandler>,
    ) -> Result<(), LintError>
    where
        F: FnMut(ModSpecification, &mut Box<dyn ReadSeek>, &PakReader) -> Result<(), LintError>,
        EmptyArchiveHandler: FnMut(ModSpecification),
        OnlyNonPakFilesHandler: FnMut(ModSpecification),
        MultiplePakFilesHandler: FnMut(ModSpecification),
    {
        for (mod_spec, mod_pak_path) in &self.mods {
            let maybe_archive_reader = Box::new(BufReader::new(fs::File::open(mod_pak_path)?));
            let bufs = match lint_get_all_files_from_data(maybe_archive_reader) {
                Ok(bufs) => bufs,
                Err(e) => match e {
                    LintError::EmptyArchive => {
                        if let Some(ref mut handler) = empty_archive_handler {
                            handler(mod_spec.clone());
                        }
                        continue;
                    }
                    LintError::OnlyNonPakFiles => {
                        if let Some(ref mut handler) = only_non_pak_files_handler {
                            handler(mod_spec.clone());
                        }
                        continue;
                    }
                    e => return Err(e),
                },
            };

            let mut individual_pak_readers = bufs
                .into_iter()
                .filter_map(|(_, pak_or_non_pak)| match pak_or_non_pak {
                    PakOrNotPak::Pak(individual_pak_reader) => Some(individual_pak_reader),
                    PakOrNotPak::NotPak => None,
                })
                .collect::<Vec<_>>();

            if individual_pak_readers.len() > 1
                && let Some(ref mut handler) = multiple_pak_files_handler
            {
                handler(mod_spec.clone());
            }

            let mut first_pak_read_seek = individual_pak_readers.remove(0);
            let pak_reader = repak::PakBuilder::new().reader(&mut first_pak_read_seek)?;
            f(mod_spec.clone(), &mut first_pak_read_seek, &pak_reader)?
        }

        Ok(())
    }

    pub fn for_each_mod_file<F>(&self, mut f: F) -> Result<(), LintError>
    where
        F: FnMut(
            ModSpecification,
            &mut Box<dyn ReadSeek>,
            &PakReader,
            PathBuf,
            String,
        ) -> Result<(), LintError>,
    {
        self.for_each_mod(
            |mod_spec, pak_read_seek, pak_reader| {
                let mount = PathBuf::from(pak_reader.mount_point());
                for p in pak_reader.files() {
                    let path = mount.join(&p);
                    let path_buf = path.strip_prefix("../../../")?;
                    let normalized_path = &path_buf.to_string_lossy().replace('\\', "/");
                    let normalized_path = normalized_path.to_ascii_lowercase();
                    f(
                        mod_spec.clone(),
                        pak_read_seek,
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

pub(crate) enum PakOrNotPak {
    Pak(Box<dyn ReadSeek>),
    NotPak,
}

pub(crate) fn lint_get_all_files_from_data(
    mut data: Box<dyn ReadSeek>,
) -> Result<Vec<(PathBuf, PakOrNotPak)>, LintError> {
    if let Ok(mut archive) = zip::ZipArchive::new(&mut data) {
        ensure!(!archive.is_empty(), EmptyArchiveSnafu);

        let mut files = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|_| LintError::ZipArchiveError)?;

            if let Some(p) = file.enclosed_name().as_deref().map(Path::to_path_buf)
                && file.is_file()
            {
                if p.extension().filter(|e| e == &"pak").is_some() {
                    let mut buf = vec![];
                    file.read_to_end(&mut buf)?;
                    files.push((
                        p.to_path_buf(),
                        PakOrNotPak::Pak(Box::new(Cursor::new(buf))),
                    ));
                } else {
                    let mut buf = vec![];
                    file.read_to_end(&mut buf)?;
                    files.push((p.to_path_buf(), PakOrNotPak::NotPak));
                }
            }
        }

        if files
            .iter()
            .filter(|(_, pak_or_not_pak)| matches!(pak_or_not_pak, PakOrNotPak::Pak(..)))
            .count()
            >= 1
        {
            Ok(files)
        } else {
            OnlyNonPakFilesSnafu.fail()?
        }
    } else {
        data.rewind()?;
        Ok(vec![(PathBuf::from("."), PakOrNotPak::Pak(data))])
    }
}

pub trait Lint {
    type Output;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output, LintError>;
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
    pub const SPLIT_ASSET_PAIRS: Self = LintId {
        name: "split_asset_pairs",
    };
    pub const UNMODIFIED_GAME_ASSETS: Self = LintId {
        name: "unmodified_game_assets",
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
    pub split_asset_pairs_mods:
        Option<BTreeMap<ModSpecification, BTreeMap<String, SplitAssetPair>>>,
    pub unmodified_game_assets_mods: Option<BTreeMap<ModSpecification, BTreeSet<String>>>,
}

pub fn run_lints(
    enabled_lints: &BTreeSet<LintId>,
    mods: IndexSet<(ModSpecification, PathBuf)>,
    fsd_pak_path: Option<PathBuf>,
) -> Result<LintReport, LintError> {
    let lint_ctxt = LintCtxt::init(mods, fsd_pak_path)?;
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
                let res = NonAssetFilesLint.check_mods(&lint_ctxt)?;
                lint_report.non_asset_file_mods = Some(res);
            }
            LintId::SPLIT_ASSET_PAIRS => {
                let res = SplitAssetPairsLint.check_mods(&lint_ctxt)?;
                lint_report.split_asset_pairs_mods = Some(res);
            }
            LintId::UNMODIFIED_GAME_ASSETS => {
                let res = UnmodifiedGameAssetsLint.check_mods(&lint_ctxt)?;
                lint_report.unmodified_game_assets_mods = Some(res);
            }
            _ => unimplemented!(),
        }
    }

    Ok(lint_report)
}
