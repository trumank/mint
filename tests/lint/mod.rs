use std::path::PathBuf;
use std::str::FromStr;

use mint::mod_lints::{LintId, LintReport, SplitAssetPair};
use mint::providers::ModSpecification;

#[test]
pub fn test_lint_conflicting_files() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let a_path = base_path.clone().join("A.pak");
    assert!(a_path.exists());
    let b_path = base_path.clone().join("B.pak");
    assert!(b_path.exists());

    let a_spec = ModSpecification {
        url: "A".to_string(),
    };
    let b_spec = ModSpecification {
        url: "B".to_string(),
    };
    let mods = [(a_spec.clone(), a_path), (b_spec.clone(), b_path)];

    let LintReport {
        conflicting_mods, ..
    } = mint::mod_lints::run_lints(&[LintId::CONFLICTING].into(), mods.into(), None).unwrap();

    println!("{:#?}", conflicting_mods);

    assert_eq!(
        conflicting_mods.unwrap().get("fsd/content/a.uexp"),
        Some(&[a_spec, b_spec].into())
    );
}

#[test]
pub fn test_lint_shader() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let a_path = base_path.clone().join("A.pak");
    assert!(a_path.exists());
    let b_path = base_path.clone().join("B.pak");
    assert!(b_path.exists());
    let a_spec = ModSpecification {
        url: "A".to_string(),
    };
    let b_spec = ModSpecification {
        url: "B".to_string(),
    };
    let mods = [(a_spec.clone(), a_path), (b_spec.clone(), b_path)];

    let LintReport {
        shader_file_mods, ..
    } = mint::mod_lints::run_lints(&[LintId::SHADER_FILES].into(), mods.into(), None).unwrap();

    println!("{:#?}", shader_file_mods);

    assert_eq!(
        shader_file_mods.unwrap().get(&a_spec),
        Some(&["fsd/content/c.ushaderbytecode".to_string()].into())
    );
}

#[test]
pub fn test_lint_asset_registry_bin() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let a_path = base_path.clone().join("A.pak");
    assert!(a_path.exists());
    let b_path = base_path.clone().join("B.pak");
    assert!(b_path.exists());
    let a_spec = ModSpecification {
        url: "A".to_string(),
    };
    let b_spec = ModSpecification {
        url: "B".to_string(),
    };
    let mods = [(a_spec.clone(), a_path), (b_spec.clone(), b_path)];

    let LintReport {
        asset_register_bin_mods,
        ..
    } = mint::mod_lints::run_lints(&[LintId::ASSET_REGISTRY_BIN].into(), mods.into(), None)
        .unwrap();

    println!("{:#?}", asset_register_bin_mods);

    assert_eq!(
        asset_register_bin_mods.unwrap().get(&a_spec),
        Some(&["fsd/content/assetregistry.bin".to_string()].into())
    );
}

#[test]
pub fn test_lint_outdated_pak_version() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let outdated_pak_path = base_path.clone().join("outdated_pak_version.pak");
    assert!(outdated_pak_path.exists());
    let outdated_spec = ModSpecification {
        url: "outdated".to_string(),
    };
    let mods = [(outdated_spec.clone(), outdated_pak_path)];

    let LintReport {
        outdated_pak_version_mods,
        ..
    } = mint::mod_lints::run_lints(&[LintId::OUTDATED_PAK_VERSION].into(), mods.into(), None)
        .unwrap();

    println!("{:#?}", outdated_pak_version_mods);

    assert_eq!(
        outdated_pak_version_mods.unwrap().get(&outdated_spec),
        Some(&repak::Version::V10)
    );
}

#[test]
pub fn test_lint_empty_archive() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let empty_archive_path = base_path.clone().join("empty_archive.zip");
    assert!(empty_archive_path.exists());
    let empty_archive_spec = ModSpecification {
        url: "empty".to_string(),
    };
    let mods = [(empty_archive_spec.clone(), empty_archive_path)];

    let LintReport {
        empty_archive_mods, ..
    } = mint::mod_lints::run_lints(&[LintId::EMPTY_ARCHIVE].into(), mods.into(), None).unwrap();

    println!("{:#?}", empty_archive_mods);

    assert!(empty_archive_mods.unwrap().contains(&empty_archive_spec));
}

#[test]
pub fn test_lint_only_non_pak_files() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let a_path = base_path.clone().join("A.pak");
    assert!(a_path.exists());
    let only_non_pak_path = base_path.clone().join("only_non_pak_files.zip");
    assert!(only_non_pak_path.exists());
    let a_spec = ModSpecification {
        url: "A".to_string(),
    };
    let only_non_pak_spec = ModSpecification {
        url: "only_non_pak".to_string(),
    };
    let mods = [
        (a_spec.clone(), a_path),
        (only_non_pak_spec.clone(), only_non_pak_path),
    ];

    let LintReport {
        archive_with_only_non_pak_files_mods,
        ..
    } = mint::mod_lints::run_lints(
        &[LintId::ARCHIVE_WITH_ONLY_NON_PAK_FILES].into(),
        mods.into(),
        None,
    )
    .unwrap();

    println!("{:#?}", archive_with_only_non_pak_files_mods);

    assert!(archive_with_only_non_pak_files_mods
        .unwrap()
        .contains(&only_non_pak_spec));
}

#[test]
pub fn test_lint_multi_pak_archive() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let multiple_paks_archive_path = base_path.clone().join("multiple_paks.zip");
    assert!(multiple_paks_archive_path.exists());
    let multiple_paks_spec = ModSpecification {
        url: "multiple_paks".to_string(),
    };
    let mods = [(multiple_paks_spec.clone(), multiple_paks_archive_path)];

    let LintReport {
        archive_with_multiple_paks_mods,
        ..
    } = mint::mod_lints::run_lints(
        &[LintId::ARCHIVE_WITH_MULTIPLE_PAKS].into(),
        mods.into(),
        None,
    )
    .unwrap();

    println!("{:#?}", archive_with_multiple_paks_mods);

    assert!(archive_with_multiple_paks_mods
        .unwrap()
        .contains(&multiple_paks_spec));
}

#[test]
pub fn test_lint_non_asset_files() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let non_asset_files_path = base_path.clone().join("non_asset_files.pak");
    assert!(non_asset_files_path.exists());
    let non_asset_files_spec = ModSpecification {
        url: "non_asset_files".to_string(),
    };
    let mods = [(non_asset_files_spec.clone(), non_asset_files_path)];

    let LintReport {
        non_asset_file_mods,
        ..
    } = mint::mod_lints::run_lints(&[LintId::NON_ASSET_FILES].into(), mods.into(), None).unwrap();

    println!("{:#?}", non_asset_file_mods);

    assert_eq!(
        non_asset_file_mods.unwrap().get(&non_asset_files_spec),
        Some(&["never_gonna_give_you_up.txt".to_string()].into())
    );
}

#[test]
pub fn test_lint_split_asset_pairs() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let split_asset_pairs_path = base_path.clone().join("split_asset_pairs.pak");
    assert!(split_asset_pairs_path.exists());
    let split_asset_pairs_spec = ModSpecification {
        url: "split_asset_pairs".to_string(),
    };
    let mods = [(split_asset_pairs_spec.clone(), split_asset_pairs_path)];

    let LintReport {
        split_asset_pairs_mods,
        ..
    } = mint::mod_lints::run_lints(&[LintId::SPLIT_ASSET_PAIRS].into(), mods.into(), None).unwrap();

    println!("{:#?}", split_asset_pairs_mods);

    assert_eq!(
        split_asset_pairs_mods.unwrap().get(&split_asset_pairs_spec),
        Some(
            &[
                (
                    "missing_uasset/a.uexp".to_string(),
                    SplitAssetPair::MissingUasset
                ),
                (
                    "missing_uexp/b.uasset".to_string(),
                    SplitAssetPair::MissingUexp
                )
            ]
            .into()
        )
    );
}

#[test]
pub fn test_lint_unmodified_game_assets() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak_path = base_path.clone().join("reference.pak");
    assert!(reference_pak_path.exists());
    let unmodified_game_assets_path = base_path.clone().join("unmodified_game_assets.pak");
    assert!(unmodified_game_assets_path.exists());
    let unmodified_game_assets_spec = ModSpecification {
        url: "unmodified_game_assets".to_string(),
    };
    let mods = [(
        unmodified_game_assets_spec.clone(),
        unmodified_game_assets_path,
    )];

    let LintReport {
        unmodified_game_assets_mods,
        ..
    } = mint::mod_lints::run_lints(
        &[LintId::UNMODIFIED_GAME_ASSETS].into(),
        mods.into(),
        Some(reference_pak_path),
    )
    .unwrap();

    println!("{:#?}", unmodified_game_assets_mods);

    assert_eq!(
        unmodified_game_assets_mods
            .unwrap()
            .get(&unmodified_game_assets_spec),
        Some(&["a.uexp".to_string(), "a.uasset".to_string()].into())
    );
}
