//! Where you have been, and what the address field suggests from it.
//!
//! One entry per page, keyed by its address without the scheme or a leading
//! `www.`, so `http://www.x.com/` and `https://x.com/` are the same place.
//! Ranking is "frecency": how often, weighted by how recently.

use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

/// Beyond this many pages, the ones visited longest ago are forgotten.
const KEEP: usize = 20_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub count: u32,
    /// Seconds since the Unix epoch.
    pub last: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub url: String,
    pub title: String,
    /// The address as the row shows it.
    pub shown: String,
}

#[derive(Default)]
pub struct History {
    visits: Vec<Visit>,
    index: HashMap<String, usize>,
    dirty: bool,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// `https://www.x.com/a/` → `x.com/a`
pub fn identity(url: &str) -> String {
    let lower = url.trim();
    let rest = lower.split_once("://").map(|(_, r)| r).unwrap_or(lower);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    rest.to_lowercase()
}

/// Only pages on the web are worth remembering.
fn is_web(url: &str) -> bool {
    Url::parse(url).map(|u| matches!(u.scheme(), "http" | "https")).unwrap_or(false)
}

impl History {
    fn file() -> PathBuf {
        store::data_dir().join("history.json")
    }

    pub fn load() -> History {
        let visits: Vec<Visit> = store::load(&Self::file());
        let mut history = History { visits, ..Default::default() };
        history.reindex();
        history
    }

    fn reindex(&mut self) {
        self.index = self.visits.iter().enumerate().map(|(i, v)| (identity(&v.url), i)).collect();
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        if let Err(err) = store::save(&Self::file(), &self.visits) {
            eprintln!("wisp: couldn't save history: {err}");
        }
    }

    pub fn record(&mut self, url: &str, title: &str) {
        self.record_at(url, title, now());
    }

    fn record_at(&mut self, url: &str, title: &str, when: i64) {
        if !is_web(url) {
            return;
        }
        let key = identity(url);
        match self.index.get(&key) {
            Some(&i) => {
                let v = &mut self.visits[i];
                v.count = v.count.saturating_add(1);
                v.last = when;
                v.url = url.to_string();
                if !title.is_empty() {
                    v.title = title.to_string();
                }
            }
            None => {
                self.visits.push(Visit { url: url.to_string(), title: title.to_string(), count: 1, last: when });
                self.index.insert(key, self.visits.len() - 1);
                if self.visits.len() > KEEP {
                    self.visits.sort_by_key(|a| std::cmp::Reverse(a.last));
                    self.visits.truncate(KEEP * 9 / 10);
                    self.reindex();
                }
            }
        }
        self.dirty = true;
    }

    /// A page told us its title after it was recorded.
    pub fn retitle(&mut self, url: &str, title: &str) {
        if title.is_empty() {
            return;
        }
        if let Some(&i) = self.index.get(&identity(url))
            && self.visits[i].title != title
        {
            self.visits[i].title = title.to_string();
            self.dirty = true;
        }
    }

    pub fn forget(&mut self, url: &str) {
        if let Some(i) = self.index.remove(&identity(url)) {
            self.visits.remove(i);
            self.reindex();
            self.dirty = true;
        }
    }

    pub fn clear(&mut self) {
        self.visits.clear();
        self.index.clear();
        self.dirty = true;
    }

    /// Everything, newest first, optionally narrowed by words.
    pub fn everything(&self, typed: &str) -> Vec<Visit> {
        let words: Vec<String> = typed.split_whitespace().map(str::to_lowercase).collect();
        let mut out: Vec<Visit> = self
            .visits
            .iter()
            .filter(|v| {
                let hay = format!("{} {}", v.title.to_lowercase(), identity(&v.url));
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .cloned()
            .collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.last));
        out
    }

    fn frecency(v: &Visit, now: i64) -> f64 {
        let days = ((now - v.last).max(0) as f64) / 86_400.0;
        (1.0 + v.count as f64).ln() / (1.0 + days / 7.0)
    }

    /// How well an address matches what was typed, or None for no match.
    fn rank(key: &str, title: &str, needle: &str) -> Option<f64> {
        if key.starts_with(needle) {
            // The front page of a site outranks its deeper pages.
            return Some(if key.contains('/') { 3.0 } else { 4.0 });
        }
        let starts_a_part = key.split(['.', '/', '-', '?', '=', '&']).any(|part| part.starts_with(needle));
        if starts_a_part {
            return Some(2.0);
        }
        let title = title.to_lowercase();
        let words: Vec<&str> = needle.split_whitespace().collect();
        if !words.is_empty() && words.iter().all(|w| title.contains(w) || key.contains(w)) {
            return Some(1.0);
        }
        None
    }

    pub fn suggestions(&self, typed: &str, limit: usize) -> Vec<Suggestion> {
        self.suggestions_at(typed, limit, now())
    }

    fn suggestions_at(&self, typed: &str, limit: usize, now: i64) -> Vec<Suggestion> {
        let needle = identity(typed);
        if needle.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(f64, usize, &Visit)> = self
            .visits
            .iter()
            .filter_map(|v| {
                let key = identity(&v.url);
                Self::rank(&key, &v.title, &needle).map(|r| (r + Self::frecency(v, now), key.len(), v))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, _, v)| Suggestion { url: v.url.clone(), title: v.title.clone(), shown: identity(&v.url) })
            .collect()
    }

    /// What to complete the field with, inline, as you type: the rest of the
    /// best address that starts with exactly what was typed. It completes to
    /// the end of the host first, since that is usually where you are going.
    pub fn completion(&self, typed: &str, options: &[Suggestion]) -> Option<String> {
        let t = typed.trim_start().to_lowercase();
        let t = t.strip_prefix("www.").unwrap_or(&t);
        if t.is_empty() || t.contains(' ') {
            return None;
        }
        for option in options {
            let shown = &option.shown;
            if shown.starts_with(t) && shown.len() > t.len() {
                let host_end = shown.find('/').unwrap_or(shown.len());
                let end = if t.len() < host_end { host_end } else { shown.len() };
                return Some(shown[..end].to_string());
            }
        }
        None
    }
}
