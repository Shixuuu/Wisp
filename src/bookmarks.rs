//! Bookmarks: folders and sites, kept in a small file. Import reads the
//! `Bookmarks` file every Chromium-family browser keeps under `~/.config`.

use crate::store;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub title: String,
    /// None for a folder.
    pub url: Option<String>,
    #[serde(default)]
    pub children: Vec<Bookmark>,
}

#[derive(Default)]
pub struct Bookmarks {
    pub roots: Vec<Bookmark>,
}

fn file() -> PathBuf {
    store::data_dir().join("bookmarks.json")
}

fn fresh_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = crate::history::now() as u64 * 1000;
    base + NEXT.fetch_add(1, Ordering::Relaxed)
}

fn contains_url(nodes: &[Bookmark], url: &str) -> bool {
    nodes
        .iter()
        .any(|node| node.url.as_deref().is_some_and(|have| same(have, url)) || contains_url(&node.children, url))
}

fn same(a: &str, b: &str) -> bool {
    crate::history::identity(a) == crate::history::identity(b)
}

impl Bookmarks {
    pub fn load() -> Bookmarks {
        Bookmarks { roots: store::load(&file()) }
    }

    fn save(&self) {
        if let Err(err) = store::save(&file(), &self.roots) {
            eprintln!("wisp: couldn't save bookmarks: {err}");
        }
    }

    pub fn count(nodes: &[Bookmark]) -> usize {
        nodes.iter().map(|n| if n.url.is_none() { Self::count(&n.children) } else { 1 }).sum()
    }

    fn find_mut(nodes: &mut [Bookmark], id: u64) -> Option<&mut Bookmark> {
        for n in nodes {
            if n.id == id {
                return Some(n);
            }
            if let Some(found) = Self::find_mut(&mut n.children, id) {
                return Some(found);
            }
        }
        None
    }

    pub fn find(&self, url: &str) -> Option<Bookmark> {
        fn walk(nodes: &[Bookmark], url: &str) -> Option<Bookmark> {
            nodes.iter().find_map(|n| match &n.url {
                Some(u) if same(u, url) => Some(n.clone()),
                Some(_) => None,
                None => walk(&n.children, url),
            })
        }
        walk(&self.roots, url)
    }

    /// The page added at the end of the list, or the bookmark already kept.
    pub fn keep(&mut self, url: &str, title: &str) -> (Bookmark, bool) {
        if let Some(kept) = self.find(url) {
            return (kept, false);
        }
        let b = Bookmark { id: fresh_id(), title: title.to_string(), url: Some(url.to_string()), children: vec![] };
        self.roots.push(b.clone());
        self.save();
        (b, true)
    }

    pub fn rename(&mut self, id: u64, title: &str) {
        if let Some(n) = Self::find_mut(&mut self.roots, id) {
            n.title = title.to_string();
            self.save();
        }
    }

    fn take(nodes: &mut Vec<Bookmark>, id: u64) -> Option<Bookmark> {
        if let Some(i) = nodes.iter().position(|n| n.id == id) {
            return Some(nodes.remove(i));
        }
        nodes.iter_mut().find_map(|n| Self::take(&mut n.children, id))
    }

    pub fn remove(&mut self, id: u64) {
        Self::take(&mut self.roots, id);
        self.save();
    }

    /// Into a folder, or to the top level with None.
    pub fn move_into(&mut self, id: u64, folder: Option<u64>) {
        let Some(node) = Self::take(&mut self.roots, id) else { return };
        match folder.and_then(|f| Self::find_mut(&mut self.roots, f)) {
            Some(f) => f.children.push(node),
            None => self.roots.push(node),
        }
        self.save();
    }

    pub fn new_folder(&mut self, title: &str) -> u64 {
        let id = fresh_id();
        self.roots.push(Bookmark { id, title: title.to_string(), url: None, children: vec![] });
        self.save();
        id
    }

    /// Every folder, with how deep it sits.
    pub fn folders(&self) -> Vec<(u64, String, usize)> {
        fn walk(nodes: &[Bookmark], depth: usize, out: &mut Vec<(u64, String, usize)>) {
            for n in nodes.iter().filter(|n| n.url.is_none()) {
                out.push((n.id, n.title.clone(), depth));
                walk(&n.children, depth + 1, out);
            }
        }
        let mut out = vec![];
        walk(&self.roots, 0, &mut out);
        out
    }

    /// The folder a bookmark is filed in, by name.
    pub fn folder_of(&self, id: u64) -> Option<String> {
        fn walk(nodes: &[Bookmark], id: u64, parent: Option<&str>) -> Option<Option<String>> {
            for n in nodes {
                if n.id == id {
                    return Some(parent.map(str::to_string));
                }
                if let Some(found) = walk(&n.children, id, Some(&n.title)) {
                    return Some(found);
                }
            }
            None
        }
        walk(&self.roots, id, None).flatten()
    }

    /// Sites matching every word, with the folders they are filed under.
    pub fn matches(&self, query: &str) -> Vec<(Bookmark, Vec<String>)> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return vec![];
        }
        fn walk(nodes: &[Bookmark], path: &mut Vec<String>, words: &[String], out: &mut Vec<(Bookmark, Vec<String>)>) {
            for n in nodes {
                let hay = format!("{} {}", n.title, n.url.clone().unwrap_or_default()).to_lowercase();
                if words.iter().all(|w| hay.contains(w)) {
                    out.push((n.clone(), path.clone()));
                }
                if n.url.is_none() {
                    path.push(n.title.clone());
                    walk(&n.children, path, words, out);
                    path.pop();
                }
            }
        }
        let mut out = vec![];
        walk(&self.roots, &mut vec![], &words, &mut out);
        out
    }

    /// Bring in every Chromium-family browser's bookmarks, folders and all.
    /// Returns (browser, how many new) for each browser found.
    pub fn bring_in(&mut self) -> Vec<(String, usize)> {
        let config = store::config_dir().parent().map(PathBuf::from).unwrap_or_default();
        let sources = [
            ("Chromium", "chromium/Default/Bookmarks"),
            ("Google Chrome", "google-chrome/Default/Bookmarks"),
            ("Brave", "BraveSoftware/Brave-Browser/Default/Bookmarks"),
            ("Vivaldi", "vivaldi/Default/Bookmarks"),
            ("Microsoft Edge", "microsoft-edge/Default/Bookmarks"),
        ];
        let mut report = vec![];
        for (name, rel) in sources {
            let Ok(bytes) = std::fs::read(config.join(rel)) else { continue };
            let Ok(root) = serde_json::from_slice::<Value>(&bytes) else { continue };
            let mut found = vec![];
            if let Some(roots) = root.get("roots").and_then(Value::as_object) {
                for node in roots.values() {
                    self.convert(node, &mut found);
                }
            }
            if Self::count(&found) == 0 {
                continue;
            }
            let before = Self::count(&self.roots);
            self.roots.push(Bookmark { id: fresh_id(), title: format!("From {name}"), url: None, children: found });
            report.push((name.to_string(), Self::count(&self.roots) - before));
        }
        self.save();
        report
    }

    fn convert(&self, node: &Value, out: &mut Vec<Bookmark>) {
        let name = node.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        match node.get("type").and_then(Value::as_str) {
            Some("url") => {
                let url = node.get("url").and_then(Value::as_str).unwrap_or_default();
                let already = self.find(url).is_some() || contains_url(out, url);
                if (url.starts_with("http://") || url.starts_with("https://")) && !already {
                    out.push(Bookmark { id: fresh_id(), title: name, url: Some(url.into()), children: vec![] });
                }
            }
            Some("folder") => {
                let mut kids = vec![];
                for child in node.get("children").and_then(Value::as_array).into_iter().flatten() {
                    self.convert(child, &mut kids);
                }
                // The bars Chromium keeps at the top are not folders you made.
                if matches!(name.as_str(), "Bookmarks bar" | "Other bookmarks" | "Mobile bookmarks") {
                    out.extend(kids);
                } else if !kids.is_empty() {
                    out.push(Bookmark { id: fresh_id(), title: name, url: None, children: kids });
                }
            }
            _ => {}
        }
    }
}
