use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::{bail, Result};
use path_slash::PathExt;
use rayon::prelude::*;
use sha2::Digest;
use tracing::trace;

use crate::open_file;
use crate::providers::ModSpecification;

use super::{Lint, LintCtxt};

#[derive(Default)]
pub struct UnmodifiedGameAssetsLint;

impl Lint for UnmodifiedGameAssetsLint {
    type Output = BTreeMap<ModSpecification, BTreeSet<String>>;

    fn check_mods(&mut self, lcx: &LintCtxt) -> Result<Self::Output> {
        let Some(game_pak_path) = &lcx.fsd_pak_path else {
            bail!("UnmodifiedGameAssetsLint requires specifying a valid game pak path");
        };

        // Adapted from
        // <https://github.com/trumank/repak/blob/a006d9ed6f021687a87b8b2ff9d66083d019824c/repak_cli/src/main.rs#L217>.
        let mut reader = BufReader::new(open_file(game_pak_path)?);
        let pak = repak::PakReader::new_any(&mut reader, None)?;

        let mount_point = PathBuf::from(pak.mount_point());

        let full_paths = pak
            .files()
            .into_iter()
            .map(|f| (mount_point.join(&f), f))
            .collect::<Vec<_>>();
        let stripped = full_paths
            .iter()
            .map(|(full_path, _path)| full_path.strip_prefix("../../../"))
            .collect::<Result<Vec<_>, _>>()?;

        let game_file_hashes: std::sync::Arc<
            std::sync::Mutex<BTreeMap<std::borrow::Cow<'_, str>, Vec<u8>>>,
        > = Default::default();

        full_paths.par_iter().zip(stripped).try_for_each_init(
            || (game_file_hashes.clone(), File::open(game_pak_path)),
            |(hashes, file), ((_full_path, path), stripped)| -> Result<(), repak::Error> {
                let mut hasher = sha2::Sha256::new();
                pak.read_file(
                    path,
                    &mut BufReader::new(file.as_ref().unwrap()),
                    &mut hasher,
                )?;
                let hash = hasher.finalize();
                hashes
                    .lock()
                    .unwrap()
                    .insert(stripped.to_slash_lossy(), hash.to_vec());
                Ok(())
            },
        )?;

        let mut unmodified_game_assets = BTreeMap::new();

        lcx.for_each_mod_file(
            |mod_spec, mut pak_read_seek, pak_reader, _, normalized_path| {
                if let Some(reference_hash) = game_file_hashes
                    .lock()
                    .unwrap()
                    .get(&Cow::Owned(normalized_path.clone()))
                {
                    let mut hasher = sha2::Sha256::new();
                    pak_reader.read_file(&normalized_path, &mut pak_read_seek, &mut hasher)?;
                    let mod_file_hash = hasher.finalize().to_vec();

                    if &mod_file_hash == reference_hash {
                        unmodified_game_assets
                            .entry(mod_spec)
                            .and_modify(|paths: &mut BTreeSet<String>| {
                                paths.insert(normalized_path.clone());
                            })
                            .or_insert_with(|| [normalized_path].into());
                    }
                }

                Ok(())
            },
        )?;

        trace!("unmodified_game_assets:\n{:#?}", unmodified_game_assets);

        Ok(unmodified_game_assets)
    }
}
