//! Settings, the search engine, and keyword shortcuts.
//!
//! Stored as one small JSON file in `~/.config/wisp/settings.json`. Every
//! field has a default, so a file from an older version still loads.

use crate::store;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    DuckDuckGo,
    Google,
    Bing,
    Ecosia,
    Startpage,
    Kagi,
    Brave,
    Qwant,
    Custom,
}

impl Engine {
    pub const ALL: [Engine; 9] = [
        Engine::DuckDuckGo,
        Engine::Google,
        Engine::Bing,
        Engine::Ecosia,
        Engine::Startpage,
        Engine::Kagi,
        Engine::Brave,
        Engine::Qwant,
        Engine::Custom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "DuckDuckGo",
            Engine::Google => "Google",
            Engine::Bing => "Bing",
            Engine::Ecosia => "Ecosia",
            Engine::Startpage => "Startpage",
            Engine::Kagi => "Kagi",
            Engine::Brave => "Brave Search",
            Engine::Qwant => "Qwant",
            Engine::Custom => "Custom",
        }
    }

    fn template(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "https://duckduckgo.com/?q=%s",
            Engine::Google => "https://www.google.com/search?q=%s",
            Engine::Bing => "https://www.bing.com/search?q=%s",
            Engine::Ecosia => "https://www.ecosia.org/search?q=%s",
            Engine::Startpage => "https://www.startpage.com/sp/search?query=%s",
            Engine::Kagi => "https://kagi.com/search?q=%s",
            Engine::Brave => "https://search.brave.com/search?q=%s",
            Engine::Qwant => "https://www.qwant.com/?q=%s",
            Engine::Custom => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// A word typed before a search: "aw pacman" goes straight to the Arch Wiki.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keyword {
    pub keyword: String,
    pub template: String,
}

fn default_keywords() -> Vec<Keyword> {
    let k = |w: &str, t: &str| Keyword { keyword: w.into(), template: t.into() };
    vec![
        k("aw", "https://wiki.archlinux.org/index.php?search=%s"),
        k("aur", "https://aur.archlinux.org/packages?K=%s"),
        k("pkg", "https://archlinux.org/packages/?q=%s"),
        k("yt", "https://www.youtube.com/results?search_query=%s"),
        k("gh", "https://github.com/search?q=%s"),
        k("w", "https://en.wikipedia.org/w/index.php?search=%s"),
        k("crate", "https://crates.io/search?q=%s"),
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub engine: Engine,
    /// Used when `engine` is `Custom`. An address with `%s` where the words go.
    pub custom_engine: String,
    pub keywords: Vec<Keyword>,
    /// Tabs down the left (true) or across the top (false).
    pub tabs_on_left: bool,
    /// The sidebar folded away, leaving only the page.
    pub sidebar_folded: bool,
    pub sidebar_width: i32,
    pub theme: Theme,
    /// The ad and tracker blocker.
    pub shield: bool,
    /// Sites the blocker is switched off for, because it broke them.
    pub shield_paused: Vec<String>,
    /// Background tabs untouched for this long give back their memory.
    /// 0 means never.
    pub sleep_minutes: u32,
    /// Bring back last session's tabs at launch.
    pub restore_session: bool,
    pub default_zoom: f64,
    /// Let WebKit draw pages on the GPU. Turn off if a driver misbehaves.
    pub hardware_acceleration: bool,
    pub smooth_scrolling: bool,
    /// Spell-check languages, e.g. ["en_US"]. Empty uses the system locale.
    pub spell_languages: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            engine: Engine::default(),
            custom_engine: String::new(),
            keywords: default_keywords(),
            tabs_on_left: true,
            sidebar_folded: false,
            sidebar_width: 240,
            theme: Theme::default(),
            shield: true,
            shield_paused: Vec::new(),
            sleep_minutes: 30,
            restore_session: true,
            default_zoom: 1.0,
            hardware_acceleration: true,
            smooth_scrolling: true,
            spell_languages: Vec::new(),
        }
    }
}

impl Settings {
    fn file() -> PathBuf {
        store::config_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        store::load(&Self::file())
    }

    pub fn save(&self) {
        if let Err(err) = store::save(&Self::file(), self) {
            eprintln!("wisp: couldn't save settings: {err}");
        }
    }

    fn engine_template(&self) -> &str {
        if self.engine == Engine::Custom && accepts(&self.custom_engine) {
            &self.custom_engine
        } else if self.engine == Engine::Custom {
            Engine::DuckDuckGo.template()
        } else {
            self.engine.template()
        }
    }

    /// Where typed words go: to a keyword's site if they start with one,
    /// otherwise to the chosen engine.
    pub fn search_url(&self, typed: &str) -> String {
        if let Some((keyword, rest)) = self.keyword_match(typed) {
            return fill(&keyword.template, rest);
        }
        fill(self.engine_template(), typed.trim())
    }

    pub fn keyword_match<'a>(&'a self, typed: &'a str) -> Option<(&'a Keyword, &'a str)> {
        let (word, rest) = typed.trim_start().split_once(' ')?;
        let rest = rest.trim();
        if rest.is_empty() {
            return None;
        }
        self.keywords
            .iter()
            .filter(|k| accepts(&k.template))
            .find(|k| !k.keyword.is_empty() && k.keyword.eq_ignore_ascii_case(word))
            .map(|k| (k, rest))
    }

    pub fn is_paused(&self, host: &str) -> bool {
        self.shield_paused.iter().any(|h| h == host)
    }

    pub fn set_paused(&mut self, host: &str, paused: bool) {
        self.shield_paused.retain(|h| h != host);
        if paused {
            self.shield_paused.push(host.to_string());
            self.shield_paused.sort();
        }
    }
}

const MARK: &str = "WISPSEARCHWORDS";

/// A template words may be sent to: http or https, one `%s`, and that `%s`
/// in the path, query or fragment. Anywhere else (in the host, before an @)
/// the words would decide where you end up rather than what you look for.
pub fn accepts(template: &str) -> bool {
    let t = template.trim();
    if t.matches("%s").count() != 1 {
        return false;
    }
    let Ok(url) = Url::parse(&t.replace("%s", MARK)) else { return false };
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return false;
    }
    if url.host_str().is_some_and(|h| h.to_ascii_uppercase().contains(MARK)) || url.username().contains(MARK) {
        return false;
    }
    [Some(url.path()), url.query(), url.fragment()].iter().flatten().any(|p| p.contains(MARK))
}

fn fill(template: &str, words: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(words.as_bytes()).collect();
    template.replacen("%s", &encoded, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_and_keywords() {
        let s = Settings::default();
        assert_eq!(s.search_url("arch linux"), "https://duckduckgo.com/?q=arch+linux");
        assert_eq!(s.search_url("aw pacman hooks"), "https://wiki.archlinux.org/index.php?search=pacman+hooks");
        assert_eq!(s.search_url("AUR paru"), "https://aur.archlinux.org/packages?K=paru");
        // A keyword with nothing after it is just a word to search for.
        assert_eq!(s.search_url("aw"), "https://duckduckgo.com/?q=aw");
    }

    #[test]
    fn templates() {
        assert!(accepts("https://example.com/?q=%s"));
        assert!(accepts("https://example.com/search/%s"));
        assert!(!accepts("https://%s.example.com/"));
        assert!(!accepts("https://example.com/?q=%s&r=%s"));
        assert!(!accepts("ftp://example.com/?q=%s"));
        assert!(!accepts("https://example.com/"));
    }

    #[test]
    fn old_file_still_loads() {
        let s: Settings = serde_json::from_str(r#"{"engine":"kagi"}"#).unwrap();
        assert_eq!(s.engine, Engine::Kagi);
        assert!(s.shield);
    }
}
