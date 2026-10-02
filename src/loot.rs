//! Downloads that finished: the last fifty, kept in `downloads.json`.
//! Clearing the list leaves the files where they are.

use crate::store;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Keep {
    pub name: String,
    pub from: String,
    pub path: String,
    pub date: i64,
}

impl Keep {
    pub fn still_there(&self) -> bool {
        std::path::Path::new(&self.path).exists()
    }
}

#[derive(Default)]
pub struct Loot {
    pub kept: Vec<Keep>,
}

fn file() -> std::path::PathBuf {
    store::data_dir().join("downloads.json")
}

impl Loot {
    pub fn load() -> Loot {
        Loot { kept: store::load(&file()) }
    }

    fn save(&self) {
        if let Err(err) = store::save(&file(), &self.kept) {
            eprintln!("wisp: couldn't save downloads: {err}");
        }
    }

    pub fn add(&mut self, keep: Keep) {
        self.kept.retain(|k| k.path != keep.path);
        self.kept.insert(0, keep);
        self.kept.truncate(50);
        self.save();
    }

    pub fn forget(&mut self, path: &str) {
        self.kept.retain(|k| k.path != path);
        self.save();
    }

    pub fn forget_all(&mut self) {
        self.kept.clear();
        self.save();
    }
}

/// `my.file.v2.zip` → `my.file.v2 (2).zip` until the name is free.
/// The file is created empty so two downloads cannot take the same name.
pub fn free_name(dir: &std::path::Path, suggested: &str) -> std::path::PathBuf {
    let clean: String = suggested.chars().map(|c| if c == '/' || c == '\0' { '_' } else { c }).collect();
    let clean = clean.trim_start_matches('.');
    let name = if clean.trim().is_empty() { "download" } else { clean };
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    for n in 0..10_000 {
        let candidate = if n == 0 { dir.join(name) } else { dir.join(format!("{stem} ({n}){ext}")) };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(_) => return candidate,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return candidate,
        }
    }
    dir.join(name)
}
