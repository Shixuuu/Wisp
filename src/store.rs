//! Where Wisp keeps its files, and how it writes them.
//!
//! Everything follows the XDG base directory spec, so on Arch it lands where
//! every other well-behaved program's files do:
//!
//! - `$XDG_DATA_HOME/wisp`   (usually `~/.local/share/wisp`): history,
//!   bookmarks, open tabs, hidden elements, WebKit's cookies and site data.
//! - `$XDG_CONFIG_HOME/wisp` (usually `~/.config/wisp`): settings.
//! - `$XDG_CACHE_HOME/wisp`  (usually `~/.cache/wisp`): WebKit's HTTP cache
//!   and the compiled ad-block list. Safe to delete at any time.
//!
//! Files are small JSON documents written atomically (write to a temporary
//! file, then rename), so a crash mid-save never leaves half a file behind.

use serde::{Serialize, de::DeserializeOwned};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const APP: &str = "wisp";

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        _ => {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
            home.join(fallback)
        }
    }
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join(APP)
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join(APP)
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join(APP)
}

pub fn downloads_dir() -> PathBuf {
    glib_download_dir().unwrap_or_else(|| xdg("HOME", "").join("Downloads"))
}

fn glib_download_dir() -> Option<PathBuf> {
    gtk::glib::user_special_dir(gtk::glib::UserDirectory::Downloads)
}

/// Read a JSON file, or fall back to the default when it is missing or broken.
/// A broken file is kept aside as `name.broken` rather than overwritten, so a
/// bug never silently eats somebody's history.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(bytes) = fs::read(path) else { return T::default() };
    match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("wisp: {} is unreadable ({err}); starting fresh", path.display());
            let _ = fs::rename(path, path.with_extension("broken"));
            T::default()
        }
    }
}

/// Write a JSON file atomically.
pub fn save<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        file.write_all(&bytes)?;
        file.sync_data()?;
    }
    fs::rename(tmp, path)
}
