//! The one field. Type an address and you go there; type words and you search.
//! It suggests from your open tabs, bookmarks and history, finishes addresses
//! inline as you type, and sends nothing anywhere until you press Enter.

use crate::address;
use crate::browser::Browser;
use gtk::prelude::*;
use gtk::{gdk, glib, pango};
use std::rc::Rc;

/// What choosing a suggestion does.
#[derive(Clone, Debug)]
pub enum Pick {
    Go(String),
    SwitchTo(u64),
    Search(String),
}

const LIMIT: usize = 7;

impl Browser {
    /// Show the current page's address, unless you are typing in the field.
    pub fn show_address(&self) {
        if self.field.has_focus() && !self.field_typed.borrow().is_empty() {
            return;
        }
        let url = self.current.borrow().as_ref().map(|t| t.url.borrow().clone()).unwrap_or_default();
        let shown = if url.is_empty() || url == "about:blank" { String::new() } else { address::editable(&url) };
        self.set_field_quietly(&shown);
        if let Some(tab) = self.current.borrow().as_ref() {
            if tab.private {
                self.field.add_css_class("private");
            } else {
                self.field.remove_css_class("private");
            }
        }
    }

    fn set_field_quietly(&self, text: &str) {
        self.field_quiet.set(true);
        self.field.set_text(text);
        self.field_quiet.set(false);
    }

    pub fn focus_field(&self) {
        self.field.grab_focus();
        self.field.select_region(0, -1);
    }

    pub fn wire_field(self: &Rc<Self>) {
        let weak = self.weak();
        self.field.connect_changed(move |field| {
            let Some(b) = weak.upgrade() else { return };
            if b.field_quiet.get() {
                return;
            }
            let text = field.text().to_string();
            let before = b.field_typed.replace(text.clone());
            // Finish the address only when you added to the end: never while
            // you are deleting, or it would put back what you just took out.
            let grew = text.len() > before.len() && text.starts_with(before.as_str());
            b.suggest(&text, grew);
        });

        let weak = self.weak();
        self.field.connect_activate(move |field| {
            let Some(b) = weak.upgrade() else { return };
            let chosen = b
                .suggestion_list
                .selected_row()
                .filter(|_| b.suggestions.is_visible())
                .and_then(|row| b.suggestion_urls.borrow().get(row.index() as usize).cloned());
            let text = field.text().to_string();
            b.close_suggestions();
            b.field_typed.replace(String::new());
            match chosen {
                Some(pick) => b.choose(pick),
                None => b.navigate(&text),
            }
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.weak();
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(b) = weak.upgrade() else { return glib::Propagation::Proceed };
            match key {
                gdk::Key::Down | gdk::Key::Up if b.suggestions.is_visible() => {
                    b.move_selection(if key == gdk::Key::Down { 1 } else { -1 });
                    glib::Propagation::Stop
                }
                gdk::Key::Escape => {
                    if b.suggestions.is_visible() {
                        b.close_suggestions();
                    } else {
                        // Put back the page's address and hand the keys back
                        // to the page.
                        b.field_typed.replace(String::new());
                        b.show_address();
                        if let Some(v) = b.current_view() {
                            v.grab_focus();
                        }
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.field.add_controller(keys);

        let focus = gtk::EventControllerFocus::new();
        let weak = self.weak();
        focus.connect_enter(move |_| {
            if let Some(b) = weak.upgrade() {
                let field = b.field.clone();
                glib::idle_add_local_once(move || field.select_region(0, -1));
            }
        });
        let weak = self.weak();
        focus.connect_leave(move |_| {
            if let Some(b) = weak.upgrade() {
                b.close_suggestions();
                b.field_typed.replace(String::new());
                b.show_address();
            }
        });
        self.field.add_controller(focus);

        let weak = self.weak();
        self.suggestion_list.connect_row_activated(move |_, row| {
            let Some(b) = weak.upgrade() else { return };
            let pick = b.suggestion_urls.borrow().get(row.index() as usize).cloned();
            b.close_suggestions();
            b.field_typed.replace(String::new());
            if let Some(pick) = pick {
                b.choose(pick);
            }
        });
    }

    fn close_suggestions(&self) {
        self.suggestions.popdown();
    }

    fn move_selection(&self, by: i32) {
        let count = self.suggestion_urls.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let at = self.suggestion_list.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = (at + by).rem_euclid(count);
        if let Some(row) = self.suggestion_list.row_at_index(next) {
            self.suggestion_list.select_row(Some(&row));
        }
    }

    /// Go to what was typed: an address if it is one, a search if it isn't.
    pub fn navigate(self: &Rc<Self>, typed: &str) {
        let typed = typed.trim();
        if typed.is_empty() {
            return;
        }
        let target = match address::url_from(typed) {
            Some(url) => url.to_string(),
            None => self.prefs.borrow().search_url(typed),
        };
        self.go(&target);
    }

    fn choose(self: &Rc<Self>, pick: Pick) {
        match pick {
            Pick::Go(url) => self.go(&url),
            Pick::Search(words) => {
                let url = self.prefs.borrow().search_url(&words);
                self.go(&url);
            }
            Pick::SwitchTo(id) => {
                // Leave an empty tab behind rather than keep it around.
                let empty = self.current.borrow().as_ref().filter(|t| t.url.borrow().is_empty()).map(|t| t.id);
                if let Some(tab) = self.tab_by_id(id) {
                    self.select(&tab);
                }
                if let Some(empty) = empty.filter(|e| *e != id) {
                    self.close_tab(empty);
                }
            }
        }
    }

    fn suggest(self: &Rc<Self>, text: &str, complete: bool) {
        let typed = text.trim();
        if typed.is_empty() {
            self.close_suggestions();
            return;
        }
        let lower = typed.to_lowercase();
        let mut rows: Vec<(String, String, &'static str, Pick)> = Vec::new();

        // Open tabs first: going back to a page you have is better than
        // opening it twice.
        let current = self.current.borrow().as_ref().map(|t| t.id);
        for tab in self.tabs.borrow().iter() {
            if Some(tab.id) == current {
                continue;
            }
            let url = tab.url.borrow().clone();
            let name = tab.name();
            if name.to_lowercase().contains(&lower) || url.to_lowercase().contains(&lower) {
                rows.push((
                    name,
                    format!("Switch to tab · {}", address::pretty(&url)),
                    "view-dual-symbolic",
                    Pick::SwitchTo(tab.id),
                ));
            }
            if rows.len() >= 2 {
                break;
            }
        }

        let history = self.history.borrow().suggestions(typed, LIMIT);
        let completion = if complete { self.history.borrow().completion(typed, &history) } else { None };

        let open_pages: Vec<String> =
            self.tabs.borrow().iter().map(|t| crate::history::identity(&t.url.borrow())).collect();
        let seen = |rows: &[(String, String, &'static str, Pick)], url: &str| {
            let key = crate::history::identity(url);
            open_pages.contains(&key)
                || rows.iter().any(|r| matches!(&r.3, Pick::Go(u) if crate::history::identity(u) == key))
        };
        for b in self.bookmarks.borrow().matching(typed).into_iter().take(2) {
            if !seen(&rows, &b.url) {
                rows.push((b.title.clone(), address::pretty(&b.url), "starred-symbolic", Pick::Go(b.url)));
            }
        }
        for s in &history {
            if seen(&rows, &s.url) {
                continue;
            }
            let title = if s.title.is_empty() { s.shown.clone() } else { s.title.clone() };
            rows.push((title, s.shown.clone(), "document-open-recent-symbolic", Pick::Go(s.url.clone())));
        }
        rows.truncate(LIMIT);

        // Keep what was typed and add the rest of the address, selected, so
        // the next key either accepts it (Enter) or replaces it.
        let completed = completion.and_then(|done| {
            let shown = if text.to_lowercase().starts_with("www.") { format!("www.{done}") } else { done };
            let rest = shown.get(text.len()..)?;
            shown.to_lowercase().starts_with(&text.to_lowercase()).then(|| format!("{text}{rest}"))
        });

        // What Enter does, always first.
        let target = completed.as_deref().unwrap_or(typed);
        let first = match address::url_from(target) {
            Some(url) => {
                (target.to_string(), "Go to address".to_string(), "go-next-symbolic", Pick::Go(url.to_string()))
            }
            None => {
                let prefs = self.prefs.borrow();
                match prefs.keyword_match(typed) {
                    Some((k, rest)) => (
                        rest.to_string(),
                        format!("Search {}", address::bare_host(&k.template).unwrap_or_default()),
                        "system-search-symbolic",
                        Pick::Search(typed.to_string()),
                    ),
                    None => (
                        typed.to_string(),
                        format!("Search {}", prefs.engine.name()),
                        "system-search-symbolic",
                        Pick::Search(typed.to_string()),
                    ),
                }
            }
        };
        rows.insert(0, first);
        self.fill_suggestions(&rows);

        if let Some(full) = completed {
            // After GTK has finished the keystroke (it moves the cursor once
            // `changed` returns), and only if nothing else was typed since.
            let weak = self.weak();
            let text = text.to_string();
            glib::idle_add_local_once(move || {
                let Some(b) = weak.upgrade() else { return };
                if b.field.text() != text.as_str() || !b.field.has_focus() {
                    return;
                }
                b.set_field_quietly(&full);
                b.field.select_region(text.chars().count() as i32, -1);
            });
        }
    }

    fn fill_suggestions(&self, rows: &[(String, String, &'static str, Pick)]) {
        while let Some(child) = self.suggestion_list.first_child() {
            self.suggestion_list.remove(&child);
        }
        let mut picks = Vec::new();
        for (title, subtitle, icon, pick) in rows {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            line.add_css_class("wisp-suggestion");
            let image = gtk::Image::from_icon_name(icon);
            image.add_css_class("dim-label");
            let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let t = gtk::Label::new(Some(title));
            t.set_xalign(0.0);
            t.set_ellipsize(pango::EllipsizeMode::End);
            let s = gtk::Label::new(Some(subtitle));
            s.set_xalign(0.0);
            s.set_ellipsize(pango::EllipsizeMode::Middle);
            s.add_css_class("dim-label");
            s.add_css_class("caption");
            text.append(&t);
            text.append(&s);
            text.set_hexpand(true);
            line.append(&image);
            line.append(&text);
            self.suggestion_list.append(&line);
            // Clicking a row must not take the focus from the field.
            if let Some(row) = self.suggestion_list.last_child() {
                row.set_focusable(false);
            }
            picks.push(pick.clone());
        }
        *self.suggestion_urls.borrow_mut() = picks;
        let width = self.field.width().max(420);
        self.suggestion_list.set_size_request(width, -1);
        if !self.suggestions.is_visible() {
            self.suggestions.popup();
            // Popping up can pull focus; the typing stays in the field.
            let field = self.field.clone();
            let pos = field.position();
            field.grab_focus_without_selecting();
            field.set_position(pos);
        }
    }
}
