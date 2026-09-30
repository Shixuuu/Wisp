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
        let _ = store::save(&file(), &self.kept);
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

/// `file.tar.gz` → `file (2).tar.gz` until the name is free.
pub fn free_name(dir: &std::path::Path, suggested: &str) -> std::path::PathBuf {
    let clean: String = suggested.chars().map(|c| if c == '/' || c == '\0' { '_' } else { c }).collect();
    let clean = clean.trim_start_matches('.');
    let name = if clean.trim().is_empty() { "download" } else { clean };
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.find('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..10_000).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(first)
}
