pub mod mod_info;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

#[derive(Debug)]
pub enum DRGInstallationType {
    Steam,
    Xbox,
}

impl DRGInstallationType {
    pub fn from_exe_path() -> Result<Self> {
        let exe_name = std::env::current_exe()
            .context("could not determine running exe")?
            .file_name()
            .context("failed to get exe path")?
            .to_string_lossy()
            .to_lowercase();
        Ok(match exe_name.as_str() {
            "fsd-win64-shipping.exe" => Self::Steam,
            "fsd-wingdk-shipping.exe" => Self::Xbox,
            _ => bail!("unrecognized exe file name: {exe_name}"),
        })
    }
}

impl DRGInstallationType {
    pub fn from_pak_path<P: AsRef<Path>>(pak: P) -> Result<Self> {
        let pak_name = pak
            .as_ref()
            .file_name()
            .context("failed to get pak file name")?
            .to_string_lossy()
            .to_lowercase();
        Ok(match pak_name.as_str() {
            "fsd-windowsnoeditor.pak" => Self::Steam,
            "fsd-wingdk.pak" => Self::Xbox,
            _ => bail!("unrecognized pak file name: {pak_name}"),
        })
    }
    pub fn binaries_directory_name(&self) -> &'static str {
        match self {
            Self::Steam => "Win64",
            Self::Xbox => "WinGDK",
        }
    }
    pub fn main_pak_name(&self) -> &'static str {
        match self {
            Self::Steam => "FSD-WindowsNoEditor.pak",
            Self::Xbox => "FSD-WinGDK.pak",
        }
    }
    pub fn hook_dll_name(&self) -> &'static str {
        match self {
            Self::Steam => "x3daudio1_7.dll",
            Self::Xbox => "d3d9.dll",
        }
    }
}

#[derive(Debug)]
pub struct DRGInstallation {
    pub root: PathBuf,
    pub installation_type: DRGInstallationType,
}

impl DRGInstallation {
    /// Returns first DRG installation found. Only supports Steam version
    /// TODO locate Xbox version
    pub fn find() -> Option<Self> {
        steamlocate::SteamDir::locate()
            .and_then(|mut steamdir| {
                steamdir
                    .app(&548430)
                    .map(|a| a.path.join("FSD/Content/Paks/FSD-WindowsNoEditor.pak"))
            })
            .and_then(|path| Self::from_pak_path(path).ok())
    }
    pub fn from_pak_path<P: AsRef<Path>>(pak: P) -> Result<Self> {
        let root = pak
            .as_ref()
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("failed to get pak parent directory")?
            .to_path_buf();
        Ok(Self {
            root,
            installation_type: DRGInstallationType::from_pak_path(pak)?,
        })
    }
    pub fn binaries_directory(&self) -> PathBuf {
        self.root
            .join("Binaries")
            .join(self.installation_type.binaries_directory_name())
    }
    pub fn paks_path(&self) -> PathBuf {
        self.root.join("Content").join("Paks")
    }
    pub fn main_pak(&self) -> PathBuf {
        self.root
            .join("Content")
            .join("Paks")
            .join(self.installation_type.main_pak_name())
    }
    pub fn modio_directory(&self) -> Option<PathBuf> {
        match self.installation_type {
            DRGInstallationType::Steam => {
                #[cfg(target_os = "windows")]
                {
                    Some(PathBuf::from("C:\\Users\\Public\\mod.io\\2475"))
                }
                #[cfg(target_os = "linux")]
                {
                    steamlocate::SteamDir::locate().map(|s| {
                        s.path.join(
                            "steamapps/compatdata/548430/pfx/drive_c/users/Public/mod.io/2475",
                        )
                    })
                }
            }
            DRGInstallationType::Xbox => None,
        }
    }
}
