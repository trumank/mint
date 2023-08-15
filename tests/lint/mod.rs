use std::path::PathBuf;
use std::str::FromStr;

use drg_mod_integration::mod_lint::{ModLintReport, SplitUasset};
use drg_mod_integration::providers::ModSpecification;

#[test]
pub fn test_lint_conflicting_files() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
    let a_path = base_path.clone().join("A.pak");
    assert!(a_path.exists());
    let b_path = base_path.clone().join("B.pak");
    assert!(b_path.exists());
    let a_spec = ModSpecification {
        url: "A".to_string(),
    };
    // a\n
    let a_hash =
        hex::decode("87428fc522803d31065e7bce3cf03fe475096631e5e07bbd7a0fde60c4cf25c7").unwrap();

    let b_spec = ModSpecification {
        url: "B".to_string(),
    };
    // b\n
    let b_hash =
        hex::decode("0263829989b6fd954f72baaf2fc64bc2e2f01d692d4de72986ea808f6e99813f").unwrap();
    let mods = vec![(a_spec.clone(), a_path), (b_spec.clone(), b_path)];

    let ModLintReport {
        conflicting_mods, ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", conflicting_mods);

    assert_eq!(
        conflicting_mods.get("fsd/content/a.uexp"),
        Some(&[(a_spec, a_hash), (b_spec, b_hash)].into())
    );
}

#[test]
pub fn test_lint_shader() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
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
    let mods = vec![(a_spec.clone(), a_path), (b_spec.clone(), b_path)];

    let ModLintReport {
        shader_file_mods, ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", shader_file_mods);

    assert_eq!(
        shader_file_mods.get(&a_spec),
        Some(&["fsd/content/c.ushaderbytecode".to_string()].into())
    );
}

#[test]
pub fn test_lint_asset_registry_bin() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
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
    let mods = vec![(a_spec.clone(), a_path), (b_spec.clone(), b_path)];

    let ModLintReport {
        asset_register_bin_mods,
        ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", asset_register_bin_mods);

    assert_eq!(
        asset_register_bin_mods.get(&a_spec),
        Some(&["fsd/content/assetregistry.bin".to_string()].into())
    );
}

#[test]
pub fn test_lint_outdated_pak_version() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
    let outdated_pak_path = base_path.clone().join("outdated_pak_version.pak");
    assert!(outdated_pak_path.exists());
    let outdated_spec = ModSpecification {
        url: "outdated".to_string(),
    };
    let mods = vec![(outdated_spec.clone(), outdated_pak_path)];

    let ModLintReport {
        outdated_pak_version_mods,
        ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", outdated_pak_version_mods);

    assert_eq!(
        outdated_pak_version_mods.get(&outdated_spec),
        Some(&repak::Version::V10)
    );
}

#[test]
pub fn test_lint_empty_archive() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
    let empty_archive_path = base_path.clone().join("empty_archive.zip");
    assert!(empty_archive_path.exists());
    let empty_archive_spec = ModSpecification {
        url: "empty".to_string(),
    };
    let mods = vec![(empty_archive_spec.clone(), empty_archive_path)];

    let ModLintReport {
        empty_archive_mods, ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", empty_archive_mods);

    assert!(empty_archive_mods.contains(&empty_archive_spec));
}

#[test]
pub fn test_lint_only_non_pak_files() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
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
    let mods = vec![
        (a_spec.clone(), a_path),
        (only_non_pak_spec.clone(), only_non_pak_path),
    ];

    let ModLintReport {
        archive_with_only_non_pak_files_mods,
        ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", archive_with_only_non_pak_files_mods);

    assert!(archive_with_only_non_pak_files_mods.contains(&only_non_pak_spec));
}

#[test]
pub fn test_lint_multi_pak_archive() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
    let multiple_paks_archive_path = base_path.clone().join("multiple_paks.zip");
    assert!(multiple_paks_archive_path.exists());
    let multiple_paks_spec = ModSpecification {
        url: "multiple_paks".to_string(),
    };
    let mods = vec![(multiple_paks_spec.clone(), multiple_paks_archive_path)];

    let ModLintReport {
        archive_with_multiple_paks_mods,
        ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", archive_with_multiple_paks_mods);

    assert!(archive_with_multiple_paks_mods.contains(&multiple_paks_spec));
}

#[test]
pub fn test_lint_non_asset_files() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
    let non_asset_files_pak_path = base_path.clone().join("non_asset_files.pak");
    assert!(non_asset_files_pak_path.exists());

    let non_asset_files_spec = ModSpecification {
        url: "non_asset_files_pak".to_string(),
    };

    let mods = vec![(non_asset_files_spec.clone(), non_asset_files_pak_path)];

    let ModLintReport {
        non_asset_file_mods,
        ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", non_asset_file_mods);

    assert_eq!(
        non_asset_file_mods.get(&non_asset_files_spec),
        Some(&["never_gonna_let_you_down.txt".to_string()].into())
    );
}

#[test]
pub fn test_lint_split_uasset_uexp_pairs() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
    let split_uasset_uexp_pak_path = base_path.clone().join("split_uasset_uexp.pak");
    assert!(split_uasset_uexp_pak_path.exists());

    let split_uasset_uexp_spec = ModSpecification {
        url: "split_uasset_uexp".to_string(),
    };

    let mods = vec![(split_uasset_uexp_spec.clone(), split_uasset_uexp_pak_path)];

    let ModLintReport {
        split_uasset_uexp_mods,
        ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", split_uasset_uexp_mods);

    assert_eq!(
        split_uasset_uexp_mods.get(&split_uasset_uexp_spec),
        Some(
            &[
                (
                    "missing_uasset/a.uexp".to_string(),
                    SplitUasset::MissingUasset
                ),
                (
                    "missing_uexp/b.uasset".to_string(),
                    SplitUasset::MissingUexp
                )
            ]
            .into()
        )
    );
}

#[test]
pub fn test_lint_unmodified_base_game_assets() {
    let base_path = PathBuf::from_str("test_assets/lints/").unwrap();
    assert!(base_path.exists());
    let reference_pak = base_path.clone().join("reference.pak");
    assert!(reference_pak.exists());
    let unmodified_game_asset_pak = base_path.clone().join("unmodified_game_asset.pak");
    assert!(unmodified_game_asset_pak.exists());

    let unmodified_game_asset_spec = ModSpecification {
        url: "unmodified_game_asset".to_string(),
    };

    let mods = vec![(
        unmodified_game_asset_spec.clone(),
        unmodified_game_asset_pak.clone(),
    )];

    let ModLintReport {
        unmodified_base_game_assets,
        ..
    } = drg_mod_integration::mod_lint::lint(&reference_pak, &mods).unwrap();

    println!("{:#?}", unmodified_base_game_assets);

    assert_eq!(
        unmodified_base_game_assets.get(&unmodified_game_asset_spec),
        Some(&["fsd/a.uasset".to_string(), "fsd/a.uexp".to_string()].into())
    );
}
