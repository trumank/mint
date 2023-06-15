pub mod error;
pub mod gui;
pub mod integrate;
pub mod providers;
pub mod state;

use std::path::PathBuf;

pub fn find_drg() -> Option<PathBuf> {
    steamlocate::SteamDir::locate()
        .and_then(|mut steamdir| steamdir.app(&548430).map(|a| a.path.to_path_buf()))
}
