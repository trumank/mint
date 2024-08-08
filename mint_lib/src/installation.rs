use std::path::{Path, PathBuf};

use crate::MintError;

#[derive(Debug)]
pub enum DRGInstallationType {
    Steam,
    Xbox,
}

fn io_err<S: AsRef<str>>(e: std::io::Error, msg: S) -> MintError {
    MintError::UnknownInstallation {
        summary: msg.as_ref().to_string(),
        details: Some(e.to_string()),
    }
}

fn bad_file_name<S: AsRef<str>>(msg: S) -> MintError {
    MintError::UnknownInstallation {
        summary: msg.as_ref().to_string(),
        details: None,
    }
}

fn unexpected_file_name<S: AsRef<str>>(msg: S, expected: &[&str], found: &str) -> MintError {
    let candidates = expected
        .iter()
        .map(|e| format!("\"{e}\""))
        .collect::<Vec<_>>();
    let candidates = candidates.join(", ");
    let expectation = format!("expected one of [{candidates}] but found \"{found}\"");

    MintError::UnknownInstallation {
        summary: msg.as_ref().to_string(),
        details: Some(expectation),
    }
}

fn mk_lowercase_file_name<P: AsRef<Path>, S: AsRef<str>>(
    path: P,
    err_msg: S,
) -> Result<String, MintError> {
    let Some(mut file_name) = path
        .as_ref()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
    else {
        return Err(bad_file_name(err_msg));
    };
    file_name.make_ascii_lowercase();
    Ok(file_name)
}

const STEAM_EXE_FILE_NAME: &str = "fsd-win64-shipping.exe";
const XBOX_EXE_FILE_NAME: &str = "fsd-wingdk-shipping.exe";

const STEAM_PAK_FILE_NAME: &str = "fsd-windowsnoeditor.pak";
const XBOX_PAK_FILE_NAME: &str = "fsd-wingdk.pak";

impl DRGInstallationType {
    pub fn from_exe_path() -> Result<Self, MintError> {
        let exe_path = std::env::current_exe()
            .map_err(|e| io_err(e, "failed to get path of current executable"))?;

        let file_name = mk_lowercase_file_name(
            exe_path,
            "unable to get file name component of executable path",
        )?;

        match file_name.as_ref() {
            STEAM_EXE_FILE_NAME => Ok(Self::Steam),
            XBOX_EXE_FILE_NAME => Ok(Self::Xbox),
            n => Err(unexpected_file_name(
                "unexpected executable file name",
                &[STEAM_EXE_FILE_NAME, XBOX_EXE_FILE_NAME],
                n,
            )),
        }
    }

    pub fn from_pak_path<P: AsRef<Path>>(pak: P) -> Result<Self, MintError> {
        let file_name = mk_lowercase_file_name(pak, "failed to get pak file name")?;
        match file_name.as_ref() {
            STEAM_PAK_FILE_NAME => Ok(Self::Steam),
            XBOX_PAK_FILE_NAME => Ok(Self::Steam),
            n => Err(unexpected_file_name(
                "unexpected pak file name",
                &[STEAM_PAK_FILE_NAME, XBOX_PAK_FILE_NAME],
                n,
            )),
        }
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
    /// Returns first DRG installation found. Only supports Steam version.
    /// TODO locate Xbox version
    pub fn find() -> Option<Self> {
        steamlocate::SteamDir::locate()
            .ok()
            .and_then(|steamdir| {
                steamdir
                    .find_app(548430)
                    .ok()
                    .flatten()
                    .map(|(app, library)| {
                        library
                            .resolve_app_dir(&app)
                            .join("FSD/Content/Paks/FSD-WindowsNoEditor.pak")
                    })
            })
            .and_then(|path| Self::from_pak_path(path).ok())
    }

    pub fn from_pak_path<P: AsRef<Path>>(pak: P) -> Result<Self, MintError> {
        let pak_root = pak
            .as_ref()
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent);

        match pak_root {
            Some(root) => Ok(Self {
                root: root.to_path_buf(),
                installation_type: DRGInstallationType::from_pak_path(pak)?,
            }),
            None => Err(MintError::UnknownInstallation {
                summary: "failed to determine pak root".to_string(),
                details: Some(format!("given path was {}", pak.as_ref().display())),
            }),
        }
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
                    steamlocate::SteamDir::locate()
                        .map(|s| {
                            s.path().join(
                                "steamapps/compatdata/548430/pfx/drive_c/users/Public/mod.io/2475",
                            )
                        })
                        .ok()
                }
                #[cfg(not(any(target_os = "windows", target_os = "linux")))]
                {
                    None // TODO
                }
            }
            DRGInstallationType::Xbox => None,
        }
    }
}
