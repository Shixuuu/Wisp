//! Taking things off a page and keeping them off.
//!
//! Point at a cookie bar or a newsletter overlay: it goes, and it is still
//! gone next time. Each site keeps a list of CSS selectors, put back as a
//! stylesheet before the page draws its first frame, so nothing is ever seen
//! appearing and vanishing.

use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Veil {
    pub selector: String,
    /// What it was, in words, so the list reads like something.
    pub label: String,
    /// How big it was and where it sat, measured when it was hidden.
    #[serde(default)]
    pub note: String,
    pub date: i64,
}

#[derive(Default)]
pub struct Curtain {
    pub by_host: BTreeMap<String, Vec<Veil>>,
}

/// The key a site's list is kept under: its host without `www.`.
pub fn host_key(url: &str) -> Option<String> {
    crate::address::bare_host(url)
}

impl Curtain {
    fn file() -> PathBuf {
        store::data_dir().join("hidden.json")
    }

    pub fn load() -> Curtain {
        Curtain { by_host: store::load(&Self::file()) }
    }

    fn save(&self) {
        if let Err(err) = store::save(&Self::file(), &self.by_host) {
            eprintln!("wisp: couldn't save hidden elements: {err}");
        }
    }

    pub fn veils(&self, host: &str) -> &[Veil] {
        self.by_host.get(host).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn hide(&mut self, host: &str, selector: &str, label: &str, note: &str) {
        let list = self.by_host.entry(host.to_string()).or_default();
        if list.iter().any(|v| v.selector == selector) {
            return;
        }
        list.push(Veil {
            selector: selector.to_string(),
            label: label.to_string(),
            note: note.to_string(),
            date: crate::history::now(),
        });
        self.save();
    }

    pub fn restore(&mut self, host: &str, selector: &str) {
        if let Some(list) = self.by_host.get_mut(host) {
            list.retain(|v| v.selector != selector);
            if list.is_empty() {
                self.by_host.remove(host);
            }
        }
        self.save();
    }

    pub fn undo(&mut self, host: &str) -> Option<Veil> {
        let list = self.by_host.get_mut(host)?;
        let last = list.pop();
        if list.is_empty() {
            self.by_host.remove(host);
        }
        self.save();
        last
    }

    pub fn restore_all(&mut self, host: &str) {
        self.by_host.remove(host);
        self.save();
    }

    /// The stylesheet for a site. Each selector stands alone in its own rule:
    /// one selector the engine can't parse would otherwise take the whole
    /// list down with it. `spared` is left out, to show what it hides.
    pub fn css_without(&self, host: &str, spared: Option<&str>) -> String {
        self.veils(host)
            .iter()
            .filter(|v| Some(v.selector.as_str()) != spared)
            .map(|v| format!("{} {{ display: none !important; }}", v.selector))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
