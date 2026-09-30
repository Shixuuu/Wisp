//! One tab: an address, a title, and — only once you open it — a web view.
//!
//! A tab restored from last session, or one that has been asleep, is just its
//! address and title. The WebView (and the web process behind it) is built the
//! first time the tab is shown, and dropped again when a background tab has
//! been left alone long enough. That is why launching with forty tabs is
//! instant and costs nothing.

use crate::address;
use gtk::prelude::*;
use gtk::{gdk, glib, pango};
use std::cell::{Cell, RefCell};
use std::time::Instant;
use webkit6::prelude::*;
use webkit6::{UserContentManager, WebView};

pub struct Tab {
    pub id: u64,
    pub private: bool,
    pub url: RefCell<String>,
    pub title: RefCell<String>,
    pub pinned: Cell<bool>,
    pub view: RefCell<Option<WebView>>,
    pub content: RefCell<Option<UserContentManager>>,
    /// When it was last the tab on screen, for putting background tabs to
    /// sleep.
    pub last_seen: Cell<Instant>,
    pub zoom: Cell<f64>,
    pub row: TabRow,
}

impl Tab {
    pub fn new(id: u64, url: &str, title: &str, pinned: bool, private: bool, zoom: f64) -> Tab {
        let tab = Tab {
            id,
            private,
            url: RefCell::new(url.to_string()),
            title: RefCell::new(title.to_string()),
            pinned: Cell::new(pinned),
            view: RefCell::new(None),
            content: RefCell::new(None),
            last_seen: Cell::new(Instant::now()),
            zoom: Cell::new(zoom),
            row: TabRow::new(private),
        };
        tab.refresh_row();
        tab
    }

    /// The name the tab goes by: its title, or its address until it has one.
    pub fn name(&self) -> String {
        let title = self.title.borrow();
        if !title.trim().is_empty() {
            return title.trim().to_string();
        }
        let url = self.url.borrow();
        if url.is_empty() {
            if self.private { "Private tab".into() } else { "New tab".into() }
        } else {
            address::pretty(&url)
        }
    }

    pub fn refresh_row(&self) {
        let name = self.name();
        self.row.label.set_label(&name);
        self.row.root.set_tooltip_text(Some(&name));
        self.row.set_pinned(self.pinned.get());
        let asleep = self.view.borrow().is_none() && !self.url.borrow().is_empty();
        if asleep {
            self.row.root.add_css_class("asleep");
        } else {
            self.row.root.remove_css_class("asleep");
        }
    }

    pub fn set_icon(&self, texture: Option<&gdk::Texture>) {
        match texture {
            Some(t) => self.row.icon.set_paintable(Some(t)),
            None => self.row.icon.set_icon_name(Some(if self.private {
                "security-high-symbolic"
            } else {
                "web-browser-symbolic"
            })),
        }
    }

    pub fn is_playing_audio(&self) -> bool {
        self.view.borrow().as_ref().is_some_and(|v| v.is_playing_audio())
    }
}

/// The row that stands for a tab in the tab list: its icon, its name, and a
/// close button that appears on hover.
pub struct TabRow {
    pub root: gtk::Box,
    pub icon: gtk::Image,
    pub label: gtk::Label,
    pub audio: gtk::Image,
    pub close: gtk::Button,
}

impl TabRow {
    fn new(private: bool) -> TabRow {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("wisp-tab");
        if private {
            root.add_css_class("private");
        }
        let icon = gtk::Image::from_icon_name(if private { "security-high-symbolic" } else { "web-browser-symbolic" });
        icon.set_pixel_size(16);
        icon.add_css_class("favicon");
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.set_single_line_mode(true);
        let audio = gtk::Image::from_icon_name("audio-volume-high-symbolic");
        audio.set_pixel_size(12);
        audio.set_visible(false);
        audio.add_css_class("dim-label");
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.add_css_class("circular");
        close.add_css_class("tab-close");
        close.set_tooltip_text(Some("Close tab (Ctrl+W)"));
        close.set_valign(gtk::Align::Center);
        root.append(&icon);
        root.append(&label);
        root.append(&audio);
        root.append(&close);
        TabRow { root, icon, label, audio, close }
    }

    fn set_pinned(&self, pinned: bool) {
        self.label.set_visible(!pinned);
        self.close.set_visible(!pinned);
        if pinned {
            self.root.add_css_class("pinned");
            self.root.set_hexpand(false);
        } else {
            self.root.remove_css_class("pinned");
        }
    }

    pub fn set_current(&self, current: bool) {
        if current {
            self.root.add_css_class("current");
        } else {
            self.root.remove_css_class("current");
        }
    }
}

/// A page shown when a load fails. Plain, readable, and in the same quiet
/// style as the rest of the browser.
pub fn error_page(title: &str, detail: &str, url: &str) -> String {
    let esc = |s: &str| glib::markup_escape_text(s).to_string();
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{t}</title>
<meta name="color-scheme" content="light dark">
<style>
body{{font:15px/1.6 system-ui,sans-serif;max-width:34em;margin:18vh auto;padding:0 24px;color:#171717;background:#fff}}
h1{{font-size:22px;font-weight:600;margin:0 0 8px}}p{{color:#666;margin:0 0 6px}}code{{font-size:13px;color:#888;word-break:break-all}}
@media (prefers-color-scheme:dark){{body{{color:#ededed;background:#1c1c1c}}p{{color:#aaa}}}}
</style></head><body><h1>{t}</h1><p>{d}</p><code>{u}</code></body></html>"#,
        t = esc(title),
        d = esc(detail),
        u = esc(url)
    )
}
