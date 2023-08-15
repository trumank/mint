use std::path::PathBuf;
use std::str::FromStr;

use drg_mod_integration::mod_lints::{LintId, LintReport};
use drg_mod_integration::providers::ModSpecification;

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
    } = drg_mod_integration::mod_lints::run_lints(&[LintId::CONFLICTING].into(), mods.into())
        .unwrap();

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
    } = drg_mod_integration::mod_lints::run_lints(&[LintId::SHADER_FILES].into(), mods.into())
        .unwrap();

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
    } = drg_mod_integration::mod_lints::run_lints(
        &[LintId::ASSET_REGISTRY_BIN].into(),
        mods.into(),
    )
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
    } = drg_mod_integration::mod_lints::run_lints(
        &[LintId::OUTDATED_PAK_VERSION].into(),
        mods.into(),
    )
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
    } = drg_mod_integration::mod_lints::run_lints(&[LintId::EMPTY_ARCHIVE].into(), mods.into())
        .unwrap();

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
    } = drg_mod_integration::mod_lints::run_lints(
        &[LintId::ARCHIVE_WITH_ONLY_NON_PAK_FILES].into(),
        mods.into(),
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
    } = drg_mod_integration::mod_lints::run_lints(
        &[LintId::ARCHIVE_WITH_MULTIPLE_PAKS].into(),
        mods.into(),
    )
    .unwrap();

    println!("{:#?}", archive_with_multiple_paks_mods);

    assert!(archive_with_multiple_paks_mods
        .unwrap()
        .contains(&multiple_paks_spec));
}
