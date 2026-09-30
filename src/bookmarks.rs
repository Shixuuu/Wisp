//! Bookmarks: a flat list you can search. Folders from an import are kept as a
//! label on each bookmark rather than as a tree to click through.
//!
//! Import reads the `Bookmarks` JSON file that every Chromium-family browser
//! keeps under `~/.config`. Nothing is sent anywhere.

use crate::store;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub folder: String,
    pub added: i64,
}

#[derive(Default)]
pub struct Bookmarks {
    pub items: Vec<Bookmark>,
}

impl Bookmarks {
    fn file() -> PathBuf {
        store::data_dir().join("bookmarks.json")
    }

    pub fn load() -> Bookmarks {
        Bookmarks { items: store::load(&Self::file()) }
    }

    pub fn save(&self) {
        if let Err(err) = store::save(&Self::file(), &self.items) {
            eprintln!("wisp: couldn't save bookmarks: {err}");
        }
    }

    pub fn contains(&self, url: &str) -> bool {
        let key = crate::history::identity(url);
        self.items.iter().any(|b| crate::history::identity(&b.url) == key)
    }

    /// Add it if it is missing, take it away if it is there. Returns whether
    /// the page is bookmarked afterwards.
    pub fn toggle(&mut self, url: &str, title: &str) -> bool {
        let key = crate::history::identity(url);
        let before = self.items.len();
        self.items.retain(|b| crate::history::identity(&b.url) != key);
        let added = self.items.len() == before;
        if added {
            self.items.push(Bookmark {
                url: url.to_string(),
                title: title.to_string(),
                folder: String::new(),
                added: crate::history::now(),
            });
        }
        self.save();
        added
    }

    pub fn remove(&mut self, url: &str) {
        self.items.retain(|b| b.url != url);
        self.save();
    }

    pub fn matching(&self, typed: &str) -> Vec<Bookmark> {
        let words: Vec<String> = typed.split_whitespace().map(str::to_lowercase).collect();
        self.items
            .iter()
            .filter(|b| {
                let hay = format!("{} {} {}", b.title, b.url, b.folder).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .cloned()
            .collect()
    }

    /// Import from every Chromium-family browser found. Returns how many new
    /// bookmarks arrived, per browser.
    pub fn import_chromium(&mut self) -> Vec<(String, usize)> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
        let Some(config) = config else { return Vec::new() };
        let sources = [
            ("Chromium", "chromium/Default/Bookmarks"),
            ("Google Chrome", "google-chrome/Default/Bookmarks"),
            ("Brave", "BraveSoftware/Brave-Browser/Default/Bookmarks"),
            ("Vivaldi", "vivaldi/Default/Bookmarks"),
            ("Microsoft Edge", "microsoft-edge/Default/Bookmarks"),
        ];
        let mut report = Vec::new();
        for (name, rel) in sources {
            let path = config.join(rel);
            if let Some(found) = read_chromium(&path) {
                let added = self.merge(found);
                report.push((name.to_string(), added));
            }
        }
        self.save();
        report
    }

    fn merge(&mut self, found: Vec<Bookmark>) -> usize {
        let mut added = 0;
        for b in found {
            if !self.contains(&b.url) {
                self.items.push(b);
                added += 1;
            }
        }
        added
    }
}

pub fn read_chromium(path: &Path) -> Option<Vec<Bookmark>> {
    let bytes = std::fs::read(path).ok()?;
    let root: Value = serde_json::from_slice(&bytes).ok()?;
    let mut out = Vec::new();
    if let Some(roots) = root.get("roots").and_then(Value::as_object) {
        for node in roots.values() {
            walk(node, "", &mut out);
        }
    }
    Some(out)
}

fn walk(node: &Value, folder: &str, out: &mut Vec<Bookmark>) {
    match node.get("type").and_then(Value::as_str) {
        Some("url") => {
            let url = node.get("url").and_then(Value::as_str).unwrap_or_default();
            if url.starts_with("http://") || url.starts_with("https://") {
                out.push(Bookmark {
                    url: url.to_string(),
                    title: node.get("name").and_then(Value::as_str).unwrap_or(url).to_string(),
                    folder: folder.to_string(),
                    added: crate::history::now(),
                });
            }
        }
        Some("folder") => {
            let name = node.get("name").and_then(Value::as_str).unwrap_or_default();
            let path = match (folder.is_empty(), name) {
                (_, "Bookmarks bar" | "Other bookmarks" | "Mobile bookmarks") => folder.to_string(),
                (true, n) => n.to_string(),
                (false, n) => format!("{folder} / {n}"),
            };
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                for child in children {
                    walk(child, &path, out);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_chromium_file() {
        let json = r#"{"roots":{"bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[
            {"type":"url","name":"Arch","url":"https://archlinux.org/"},
            {"type":"folder","name":"Dev","children":[{"type":"url","name":"Rust","url":"https://rust-lang.org/"},
            {"type":"url","name":"js","url":"javascript:alert(1)"}]}]},
            "other":{"type":"folder","name":"Other bookmarks","children":[]}}}"#;
        let dir = std::env::temp_dir().join(format!("wisp-bm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Bookmarks");
        std::fs::write(&file, json).unwrap();
        let found = read_chromium(&file).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].folder, "Dev");
        let mut b = Bookmarks::default();
        assert_eq!(b.merge(found.clone()), 2);
        assert_eq!(b.merge(found), 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
