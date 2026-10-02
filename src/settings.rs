//! Settings: one small JSON file in `~/.config/wisp/settings.json`, with
//! Search's own defaults. Every field has a default, so an older file loads.

use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Google,
    DuckDuckGo,
    Bing,
    Ecosia,
    Startpage,
    Kagi,
    Brave,
    Qwant,
    Custom,
    /// An engine name this version does not know. Treated as Google.
    #[serde(other)]
    Other,
}

impl Engine {
    pub const ALL: [Engine; 9] = [
        Engine::Google,
        Engine::DuckDuckGo,
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
            Engine::Google => "Google",
            Engine::DuckDuckGo => "DuckDuckGo",
            Engine::Bing => "Bing",
            Engine::Ecosia => "Ecosia",
            Engine::Startpage => "Startpage",
            Engine::Kagi => "Kagi",
            Engine::Brave => "Brave Search",
            Engine::Qwant => "Qwant",
            Engine::Custom => "Custom",
            Engine::Other => "Google",
        }
    }

    fn template(self) -> &'static str {
        match self {
            Engine::Google => "https://www.google.com/search?q=%s",
            Engine::DuckDuckGo => "https://duckduckgo.com/?q=%s",
            Engine::Bing => "https://www.bing.com/search?q=%s",
            Engine::Ecosia => "https://www.ecosia.org/search?q=%s",
            Engine::Startpage => "https://www.startpage.com/sp/search?query=%s",
            Engine::Kagi => "https://kagi.com/search?q=%s",
            Engine::Brave => "https://search.brave.com/search?q=%s",
            Engine::Qwant => "https://www.qwant.com/?q=%s",
            Engine::Custom => "",
            Engine::Other => Engine::Google.template(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Look {
    Light,
    Dark,
    #[default]
    System,
    #[serde(other)]
    Other,
}

/// A word typed before a search: "aw pacman" goes straight to the Arch Wiki.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keyword {
    pub keyword: String,
    pub template: String,
}

fn arch_keywords() -> Vec<Keyword> {
    let k = |w: &str, t: &str| Keyword { keyword: w.into(), template: t.into() };
    vec![
        k("aw", "https://wiki.archlinux.org/index.php?search=%s"),
        k("aur", "https://aur.archlinux.org/packages?K=%s"),
        k("pkg", "https://archlinux.org/packages/?q=%s"),
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub look: Look,
    /// Tabs in a column down the side, or in a strip across the top.
    pub sidebar: bool,
    pub side_right: bool,
    /// The column hidden until the pointer reaches the window's edge.
    pub side_hides: bool,
    pub side_width: f64,
    /// Back, forward and reload before the tabs in the strip.
    pub navigation_left: bool,
    pub engine: Engine,
    pub custom_engine: String,
    pub keywords: Vec<Keyword>,
    pub sleeps_tabs: bool,
    /// A link opened behind the page waits to load until you go to it.
    pub lazy_tabs: bool,
    /// Each launch starts with the pins, not last time's tabs.
    pub starts_fresh: bool,
    /// The tab you're on fills with grey as you read down the page.
    pub shows_reading: bool,
    pub shows_links: bool,
    /// "settings", "history"… typed alone go there instead of searching.
    pub command_bar: bool,
    pub shielded: bool,
    pub shield_paused: Vec<String>,
    /// Off: WebKit's cross-site tracking prevention is on, as in Safari.
    pub keeps_sign_ins: bool,
    pub site_notifications: bool,
    pub page_zoom: f64,
    /// Zoom chosen with Ctrl+plus and Ctrl+minus, remembered per site.
    pub zooms: BTreeMap<String, f64>,
    /// Answers to camera, microphone, location and notifications, as
    /// "host kind" → allowed.
    pub permissions: BTreeMap<String, bool>,
    pub always_shows_downloads: bool,
    pub hardware_acceleration: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            look: Look::System,
            sidebar: true,
            side_right: false,
            side_hides: false,
            side_width: SIDE,
            navigation_left: false,
            engine: Engine::Google,
            custom_engine: String::new(),
            keywords: arch_keywords(),
            sleeps_tabs: true,
            lazy_tabs: false,
            starts_fresh: false,
            shows_reading: true,
            shows_links: true,
            command_bar: false,
            shielded: true,
            shield_paused: Vec::new(),
            keeps_sign_ins: false,
            site_notifications: true,
            page_zoom: 1.0,
            zooms: BTreeMap::new(),
            permissions: BTreeMap::new(),
            always_shows_downloads: false,
            hardware_acceleration: true,
        }
    }
}

fn clamp_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() { zoom.clamp(0.3, 5.0) } else { 1.0 }
}

pub const SIDE: f64 = 232.0;
pub const SIDE_MIN: f64 = 176.0;
pub const SIDE_MAX: f64 = 440.0;

impl Prefs {
    fn file() -> std::path::PathBuf {
        store::config_dir().join("settings.json")
    }

    pub fn load() -> Prefs {
        let mut p: Prefs = store::load(&Self::file());
        p.side_width = p.side_width.clamp(SIDE_MIN, SIDE_MAX);
        p.page_zoom = clamp_zoom(p.page_zoom);
        p.zooms.retain(|_, zoom| zoom.is_finite());
        for zoom in p.zooms.values_mut() {
            *zoom = clamp_zoom(*zoom);
        }
        if p.engine == Engine::Other {
            p.engine = Engine::Google;
        }
        if p.look == Look::Other {
            p.look = Look::System;
        }
        p
    }

    pub fn save(&self) {
        if let Err(err) = store::save(&Self::file(), self) {
            eprintln!("wisp: couldn't save settings: {err}");
        }
    }

    fn engine_template(&self) -> &str {
        match self.engine {
            Engine::Custom if accepts(&self.custom_engine) => &self.custom_engine,
            Engine::Custom => Engine::Google.template(),
            other => other.template(),
        }
    }

    pub fn engine_name(&self) -> String {
        match self.engine {
            Engine::Custom => crate::address::bare_host(&self.custom_engine).unwrap_or_else(|| "Custom".into()),
            other => other.name().into(),
        }
    }

    pub fn search_url(&self, words: &str) -> String {
        fill(self.engine_template(), words.trim())
    }

    /// "aw pacman" → the keyword's name and where it sends the words.
    pub fn keyword_url(&self, typed: &str) -> Option<(String, String)> {
        let (word, rest) = typed.trim_start().split_once(' ')?;
        let rest = rest.trim();
        if rest.is_empty() {
            return None;
        }
        let k = self.keywords.iter().filter(|k| accepts(&k.template)).find(|k| k.keyword.eq_ignore_ascii_case(word))?;
        Some((crate::address::bare_host(&k.template).unwrap_or_default(), fill(&k.template, rest)))
    }

    /// Where Return goes with what was typed: a place, a keyword's site, or
    /// the search engine.
    pub fn destination(&self, typed: &str) -> Option<String> {
        if typed.trim().is_empty() {
            return None;
        }
        if let Some(url) = crate::address::url_from(typed) {
            return Some(url.to_string());
        }
        if let Some((_, url)) = self.keyword_url(typed) {
            return Some(url);
        }
        Some(self.search_url(typed))
    }

    pub fn is_paused(&self, host: &str) -> bool {
        self.shield_paused.iter().any(|h| h == host)
    }

    pub fn set_paused(&mut self, host: &str, paused: bool) {
        self.shield_paused.retain(|h| h != host);
        if paused {
            self.shield_paused.push(host.to_string());
        }
    }
}

const MARK: &str = "WISPSEARCHWORDS";

/// A template words may be sent to: http or https, one `%s`, and that `%s`
/// in the path, query or fragment, never in the host.
pub fn accepts(template: &str) -> bool {
    let t = template.trim();
    if t.matches("%s").count() != 1 {
        return false;
    }
    let Ok(url) = Url::parse(&t.replace("%s", MARK)) else { return false };
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(|h| h.to_uppercase().contains(MARK)) {
        return false;
    }
    [Some(url.path()), url.query(), url.fragment()].iter().flatten().any(|p| p.contains(MARK))
}

fn fill(template: &str, words: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(words.as_bytes()).collect();
    template.replacen("%s", &encoded, 1)
}
