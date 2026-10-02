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
/// A broken file is kept aside rather than overwritten, so a bug never
/// silently eats somebody's history. A file that cannot be read is left in
/// place, and [`save`] refuses to replace it.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return T::default(),
        Err(err) => {
            eprintln!("wisp: {} could not be read ({err}); leaving it untouched", path.display());
            return T::default();
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("wisp: {} is unreadable ({err}); starting fresh", path.display());
            park_broken(path);
            T::default()
        }
    }
}

/// Move a broken file aside. A second broken copy gets its own name, so the
/// first one is still there.
fn park_broken(path: &Path) {
    let broken = path.with_extension("broken");
    let dest = if broken.exists() {
        let stamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        path.with_extension(format!("broken.{stamp}"))
    } else {
        broken
    };
    let _ = fs::rename(path, dest);
}

/// Write a JSON file atomically.
pub fn save<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if path.exists() && fs::read(path).is_err() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "refusing to replace a file that could not be read",
        ));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    struct Note {
        word: String,
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wisp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_failed_save_leaves_the_previous_file() {
        let dir = scratch("save");
        let path = dir.join("history.json");
        save(&path, &Note { word: "kept".into() }).unwrap();
        let before = fs::read(&path).unwrap();
        // A mode of 000 is still readable by root, and CI runs as root.
        // A directory is unreadable for every user. Save must refuse it
        // and leave the previous bytes where they are.
        let kept = dir.join("history.kept");
        fs::rename(&path, &kept).unwrap();
        fs::create_dir(&path).unwrap();
        let err = save(&path, &Note { word: "gone".into() }).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(path.is_dir());
        assert_eq!(fs::read(&kept).unwrap(), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_broken_copy_keeps_the_first() {
        let dir = scratch("broken");
        let path = dir.join("history.json");
        fs::write(&path, b"not json").unwrap();
        let _: Note = load(&path);
        let parked = path.with_extension("broken");
        let first = fs::read(&parked).unwrap();
        assert_eq!(first, b"not json");
        fs::write(&path, b"also bad").unwrap();
        let _: Note = load(&path);
        assert_eq!(fs::read(&parked).unwrap(), first);
        let extras = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("history.broken."))
            .count();
        assert_eq!(extras, 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
