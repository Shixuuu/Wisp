//! One field at the top of the page, and the few places it thinks you mean.
//! Raised over a page by Ctrl+L, standing on its own on a blank tab, and,
//! with Ctrl+K, a list of what is open and nothing else.

use crate::address;
use crate::browser::Browser;
use crate::motion::{self, Curve, Slide, Tween};
use adw::prelude::*;
use gtk::{gdk, glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Place,
    Search,
    Open(u64),
    Command(&'static str),
}

#[derive(Clone, Debug)]
struct Offer {
    key: String,
    title: String,
    url: String,
    kind: Kind,
}

/// Words that, typed alone, go somewhere (Settings › General › Address bar
/// commands).
const COMMANDS: &[(&str, &str)] = &[
    ("settings", "win.settings"),
    ("preferences", "win.settings"),
    ("new tab", "win.new-tab"),
    ("new private tab", "win.new-private-tab"),
    ("private tab", "win.new-private-tab"),
    ("bookmarks", "win.bookmarks"),
    ("history", "win.history"),
    ("downloads", "win.downloads"),
    ("toggle sidebar", "win.toggle-sidebar"),
    ("sidebar", "win.toggle-sidebar"),
];

pub struct Omnibox {
    b: Weak<Browser>,
    place: motion::Place,
    dim: gtk::Box,
    shaker: Slide,
    column: Slide,
    frame: gtk::Box,
    pub text: gtk::Text,
    list: gtk::Box,
    list_presence: motion::Presence,
    shown: Tween,
    /// Width of the pile. One spring, so a new keystroke continues it.
    width: Tween,
    offers: RefCell<Vec<Offer>>,
    picked: Cell<Option<usize>>,
    typed: RefCell<String>,
    ending: RefCell<Option<String>>,
    editing: Cell<bool>,
    summoning: Cell<bool>,
    pub cycling: Cell<bool>,
    quiet: Cell<bool>,
    deleting: Cell<bool>,
    glow: gtk::Box,
    breathing: RefCell<Option<glib::SourceId>>,
}

impl Omnibox {
    pub fn new(b: &Rc<Browser>) -> Rc<Omnibox> {
        let dim = gtk::Box::new(gtk::Orientation::Vertical, 0);
        dim.add_css_class("dim-page");
        dim.set_hexpand(true);
        dim.set_vexpand(true);

        let text = crate::text_box("Address");
        text.set_placeholder_text(Some("Enter a web address"));
        text.add_css_class("address");
        text.set_hexpand(true);
        text.set_input_purpose(gtk::InputPurpose::Url);
        let frame = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        frame.add_css_class("field");
        frame.append(&text);
        // The glow behind the field, breathing slowly while it is up.
        let glow = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        glow.add_css_class("breath");
        let breath = gtk::Overlay::new();
        breath.set_child(Some(&glow));
        breath.add_overlay(&frame);
        breath.set_measure_overlay(&frame, true);
        let shaker = Slide::new(&breath);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list.add_css_class("offers");
        list.set_overflow(gtk::Overflow::Visible);
        let list_presence = motion::Presence::new(&list, 1.0, 0.0);
        list_presence.slide.set_anchor(0.5, 0.0);
        list_presence.root.set_valign(gtk::Align::Start);
        list_presence.root.set_margin_top(8);
        list_presence.root.set_hexpand(true);
        list_presence.root.set_overflow(gtk::Overflow::Visible);
        list_presence.slide.set_overflow(gtk::Overflow::Visible);

        let pile = gtk::Box::new(gtk::Orientation::Vertical, 0);
        pile.set_size_request(560, -1);
        pile.set_halign(gtk::Align::Center);
        pile.set_valign(gtk::Align::Start);
        pile.set_overflow(gtk::Overflow::Visible);
        pile.append(&shaker);
        pile.append(&list_presence.root);
        let column = Slide::new(&pile);
        column.set_halign(gtk::Align::Center);
        column.set_valign(gtk::Align::Start);
        column.set_margin_top(12);
        column.set_overflow(gtk::Overflow::Visible);

        let layer = gtk::Overlay::new();
        layer.set_child(Some(&dim));
        layer.add_overlay(&column);
        layer.set_vexpand(true);
        layer.set_overflow(gtk::Overflow::Visible);
        let place = motion::Place::new(&layer);
        b.page.add_overlay(&place.root);

        let (d, c, p) = (dim.clone(), column.clone(), place.clone());
        let shown = Tween::new(&layer, 0.0, move |t| {
            if t <= 0.001 {
                p.settle();
            }
            d.set_opacity(t.clamp(0.0, 1.0));
            c.set_opacity(t.clamp(0.0, 1.0));
        });
        let pile_width = pile.clone();
        let width = Tween::new(&pile, 560.0, move |w| {
            pile_width.set_size_request(w.round() as i32, -1);
        });

        let field = Rc::new(Omnibox {
            b: b.weak(),
            place,
            dim,
            shaker,
            column,
            frame,
            text,
            list,
            list_presence,
            shown,
            width,
            offers: RefCell::default(),
            picked: Cell::new(None),
            typed: RefCell::default(),
            ending: RefCell::default(),
            editing: Cell::new(false),
            summoning: Cell::new(false),
            cycling: Cell::new(false),
            quiet: Cell::new(false),
            deleting: Cell::new(false),
            glow,
            breathing: RefCell::default(),
        });
        field.wire();
        field
    }

    fn browser(&self) -> Option<Rc<Browser>> {
        self.b.upgrade()
    }

    fn wire(self: &Rc<Self>) {
        if let Some(layer) = self.column.parent() {
            let me = Rc::downgrade(self);
            layer.connect_notify_local(Some("width"), move |_, _| {
                let Some(me) = me.upgrade() else { return };
                if me.showing() {
                    me.fit();
                }
            });
        }
        let me = Rc::downgrade(self);
        self.text.connect_changed(move |t| {
            let Some(me) = me.upgrade() else { return };
            if me.quiet.get() {
                return;
            }
            *me.typed.borrow_mut() = t.text().to_string();
            me.frame.remove_css_class("refused");
            if me.deleting.replace(false) {
                me.ending.take();
                me.guess();
                me.ending.take();
                me.fit();
                return;
            }
            me.guess();
            me.fit();
            let ending = me.ending.borrow().clone();
            if let Some(ending) = ending.filter(|e| !e.is_empty()) {
                // After GTK has finished the keystroke, and only if nothing
                // else was typed since.
                let me = me.clone();
                let typed = me.typed.borrow().clone();
                glib::idle_add_local_once(move || {
                    if me.text.text() != typed.as_str() {
                        return;
                    }
                    me.set_text(&format!("{typed}{ending}"));
                    me.text.select_region(typed.chars().count() as i32, -1);
                });
            }
        });

        let me = Rc::downgrade(self);
        self.text.connect_activate(move |_| {
            if let Some(me) = me.upgrade() {
                me.submit(false, false);
            }
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, mods| {
            let Some(me) = me.upgrade() else { return glib::Propagation::Proceed };
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            match key {
                gdk::Key::Down => me.walk(1),
                gdk::Key::Up => me.walk(-1),
                gdk::Key::Tab | gdk::Key::ISO_Left_Tab if !ctrl => {
                    if me.ending.borrow().is_some() && key == gdk::Key::Tab {
                        me.accept_ending();
                    } else if !me.offers.borrow().is_empty() {
                        me.walk(if key == gdk::Key::Tab { 1 } else { -1 });
                    }
                }
                gdk::Key::Right
                    if me.ending.borrow().is_some()
                        && me.text.position() >= me.typed.borrow().chars().count() as i32 =>
                {
                    me.accept_ending()
                }
                gdk::Key::BackSpace | gdk::Key::Delete => {
                    me.deleting.set(true);
                    return glib::Propagation::Proceed;
                }
                gdk::Key::Return | gdk::Key::KP_Enter if ctrl => {
                    me.submit(true, mods.contains(gdk::ModifierType::SHIFT_MASK))
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.text.add_controller(keys);

        let click = gtk::GestureClick::new();
        let me = Rc::downgrade(self);
        click.connect_released(move |_, _, _, _| {
            if let Some(me) = me.upgrade() {
                me.dismiss();
            }
        });
        self.dim.add_controller(click);
    }

    fn set_text(&self, text: &str) {
        self.quiet.set(true);
        self.text.set_text(text);
        self.quiet.set(false);
    }

    pub fn typed(&self) -> String {
        self.typed.borrow().clone()
    }

    pub fn showing(&self) -> bool {
        self.shown.target() > 0.5
    }

    pub fn summoning(&self) -> bool {
        self.summoning.get()
    }

    /// Put the field up, over the page or on its own.
    fn show(&self, over: bool) {
        // Not hidden: a hidden widget throws off GTK 4.14's accessibility tree.
        if over {
            self.dim.add_css_class("dim-page");
        } else {
            self.dim.remove_css_class("dim-page");
        }
        self.dim.set_can_target(over);
        self.place.show(true);
        self.shown.to(1.0, Curve::Settle);
        self.fit();
        self.breathe(true);
        let text = self.text.clone();
        glib::idle_add_local_once(move || {
            text.grab_focus_without_selecting();
        });
    }

    /// 2.6 s in, 2.6 s out. Stepped a dozen times a second rather than every
    /// frame: a glow that never rests would keep GTK drawing without pause,
    /// and its idle work, the accessibility tree among it, would wait forever.
    fn breathe(&self, on: bool) {
        if let Some(id) = self.breathing.take() {
            id.remove();
        }
        if !on {
            return;
        }
        let glow = self.glow.clone();
        let start = std::time::Instant::now();
        let tick = move || {
            let phase = (start.elapsed().as_secs_f64() / 2.6) % 2.0;
            let t = if phase > 1.0 { 2.0 - phase } else { phase };
            let eased = 0.5 - (t * std::f64::consts::PI).cos() / 2.0;
            glow.set_opacity(0.64 + 0.36 * eased);
        };
        tick();
        *self.breathing.borrow_mut() = Some(glib::timeout_add_local(Duration::from_millis(80), move || {
            tick();
            glib::ControlFlow::Continue
        }));
    }

    fn hide(&self) {
        self.breathe(false);
        self.place.release();
        self.shown.to(0.0, Curve::Quick);
        self.list_presence.show(false);
        if let Some(b) = self.browser()
            && let Some(v) = b.active().and_then(|t| t.view.borrow().clone())
        {
            v.grab_focus();
        }
    }

    /// A tab came forward: a blank one shows the field with what was typed
    /// there before; any other puts it away.
    pub fn follow(&self, tab: &crate::tab::Tab) {
        self.editing.set(false);
        self.summoning.set(false);
        self.cycling.set(false);
        self.frame.remove_css_class("refused");
        if tab.is_blank() {
            let draft = tab.draft.borrow().clone();
            *self.typed.borrow_mut() = draft.clone();
            self.set_text(&draft);
            self.offers.borrow_mut().clear();
            self.render();
            self.show(false);
            self.text.set_position(-1);
        } else {
            self.typed.borrow_mut().clear();
            self.set_text("");
            self.hide();
        }
    }

    /// Ctrl+L. The address comes up selected, so typing replaces it.
    pub fn edit(&self) {
        let Some(b) = self.browser() else { return };
        let Some(tab) = b.active() else { return };
        self.summoning.set(false);
        self.editing.set(true);
        let url = tab.address();
        *self.typed.borrow_mut() = url.clone();
        self.set_text(&url);
        self.offers.borrow_mut().clear();
        self.ending.take();
        self.picked.set(None);
        self.render();
        self.show(!tab.is_blank());
        let text = self.text.clone();
        glib::idle_add_local_once(move || {
            text.grab_focus();
            text.select_region(0, -1);
        });
    }

    /// Ctrl+K. Only what is open, the most recent first, already picked.
    pub fn summon(&self) {
        let Some(b) = self.browser() else { return };
        let over = b.active().is_some_and(|t| !t.is_blank());
        self.summoning.set(true);
        self.editing.set(true);
        self.typed.borrow_mut().clear();
        self.set_text("");
        self.guess();
        self.show(over);
    }

    /// Ctrl+K again with Ctrl still down: one step further down the list.
    pub fn step_summon(&self) {
        self.cycling.set(true);
        self.walk(1);
    }

    /// Ctrl let go of: take wherever the walk stopped.
    pub fn land(&self) {
        if self.cycling.replace(false) && self.picked.get().is_some() {
            self.submit(false, false);
        }
    }

    /// Escape: the list first, then the field. True when it did something.
    pub fn escape(&self) -> bool {
        let Some(b) = self.browser() else { return false };
        let Some(tab) = b.active() else { return false };
        if !self.showing() {
            return false;
        }
        if self.picked.get().is_some() {
            self.picked.set(None);
            self.set_text(&self.typed());
            self.render();
            return true;
        }
        if tab.is_blank() {
            // A new tab never sent anywhere: Escape takes it away, back to
            // the tab touched last.
            if self.typed.borrow().is_empty() {
                let back = b.tabs.borrow().iter().filter(|t| t.id != tab.id).max_by_key(|t| t.touched.get()).cloned();
                if let Some(back) = back {
                    b.select(&back);
                    b.close(&tab);
                    return true;
                }
            }
            return false;
        }
        self.dismiss();
        true
    }

    pub fn dismiss(&self) {
        let blank = self.browser().and_then(|b| b.active()).is_none_or(|t| t.is_blank());
        self.summoning.set(false);
        self.cycling.set(false);
        if blank {
            return;
        }
        self.editing.set(false);
        self.typed.borrow_mut().clear();
        self.hide();
    }

    fn accept_ending(&self) {
        if let Some(ending) = self.ending.take() {
            let full = format!("{}{ending}", self.typed.borrow());
            *self.typed.borrow_mut() = full.clone();
            self.set_text(&full);
            self.text.set_position(-1);
            self.guess();
            self.ending.take();
        }
    }

    /// The arrow keys walk the list; walking off the top lets go of it.
    fn walk(&self, step: isize) {
        let n = self.offers.borrow().len() as isize;
        if n == 0 {
            return;
        }
        let next = match self.picked.get() {
            None => Some(if step > 0 { 0 } else { n - 1 }),
            Some(here) => {
                let next = here as isize + step;
                (0..n).contains(&next).then_some(next)
            }
        };
        self.picked.set(next.map(|i| i as usize));
        let shown = match next {
            Some(i) => {
                let offer = self.offers.borrow()[i as usize].clone();
                if matches!(offer.kind, Kind::Open(_)) { self.typed() } else { offer.key }
            }
            None => self.typed(),
        };
        if !self.summoning.get() {
            self.set_text(&shown);
            self.text.set_position(-1);
        }
        self.render();
    }

    /// What it thinks you mean: three places you have been and, when it
    /// can't be a place, a search.
    fn guess(&self) {
        let Some(b) = self.browser() else { return };
        let typed = self.typed();
        let words = typed.trim();
        let mut offers = vec![];
        if self.summoning.get() {
            let needle = words.to_lowercase();
            let mut open: Vec<_> = b
                .tabs
                .borrow()
                .iter()
                .filter(|t| !b.is_active(t) && !t.is_blank())
                .filter(|t| {
                    needle.is_empty() || t.label().to_lowercase().contains(&needle) || t.address().contains(&needle)
                })
                .cloned()
                .collect();
            open.sort_by_key(|t| std::cmp::Reverse(t.touched.get()));
            for t in open.into_iter().take(if needle.is_empty() { 6 } else { 3 }) {
                offers.push(Offer {
                    key: t.label(),
                    title: address::pretty(&t.address()),
                    url: t.address(),
                    kind: Kind::Open(t.id),
                });
            }
            let any = !offers.is_empty();
            *self.offers.borrow_mut() = offers;
            self.ending.take();
            self.picked.set(any.then_some(0));
            self.render();
            return;
        }
        if words.is_empty() {
            self.offers.borrow_mut().clear();
            self.ending.take();
            self.picked.set(None);
            self.render();
            return;
        }
        for s in b.history.borrow().suggestions(words, 3) {
            offers.push(Offer { key: s.shown.clone(), title: s.title.clone(), url: s.url.clone(), kind: Kind::Place });
        }
        if address::url_from(words).is_none() {
            let prefs = b.prefs.borrow();
            let (title, url) =
                prefs.keyword_url(words).unwrap_or_else(|| (prefs.engine_name(), prefs.search_url(words)));
            offers.push(Offer { key: typed.clone(), title, url, kind: Kind::Search });
        }
        let command = b
            .prefs
            .borrow()
            .command_bar
            .then(|| COMMANDS.iter().find(|(word, _)| *word == words.to_lowercase()))
            .flatten();
        if let Some((word, action)) = command {
            let mut title: String = word.to_string();
            if let Some(first) = title.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            offers
                .insert(0, Offer { key: title, title: String::new(), url: String::new(), kind: Kind::Command(action) });
        }
        let places: Vec<crate::history::Suggestion> = offers
            .iter()
            .filter(|o| o.kind == Kind::Place)
            .map(|o| crate::history::Suggestion { url: o.url.clone(), title: o.title.clone(), shown: o.key.clone() })
            .collect();
        *self.ending.borrow_mut() = if command.is_some() {
            None
        } else {
            b.history.borrow().completion(&typed, &places).and_then(|done| {
                let full = if typed.to_lowercase().starts_with("www.") { format!("www.{done}") } else { done };
                full.to_lowercase()
                    .starts_with(&typed.to_lowercase())
                    .then(|| full.get(typed.len()..).map(str::to_string))
                    .flatten()
            })
        };
        *self.offers.borrow_mut() = offers;
        self.picked.set(None);
        self.render();
    }

    fn render(&self) {
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let offers = self.offers.borrow().clone();
        // Fade in only as the list opens. Arrow keys and later keystrokes rebuild
        // the same list, and replaying the fade makes every row blink.
        let arrive = !offers.is_empty() && !self.list_presence.shown();
        for (i, offer) in offers.iter().enumerate() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row.add_css_class("offer");
            if self.picked.get() == Some(i) {
                row.add_css_class("picked");
            }
            match offer.kind {
                Kind::Search => {
                    let glass = gtk::Image::from_icon_name("system-search-symbolic");
                    glass.set_pixel_size(12);
                    glass.add_css_class("muted");
                    row.append(&glass);
                }
                Kind::Open(_) => {
                    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                    dot.add_css_class("open-dot");
                    dot.set_valign(gtk::Align::Center);
                    row.append(&dot);
                }
                Kind::Command(_) => {
                    let key = gtk::Image::from_icon_name("input-keyboard-symbolic");
                    key.set_pixel_size(12);
                    key.add_css_class("muted");
                    row.append(&key);
                }
                Kind::Place => {}
            }
            let key = gtk::Label::new(Some(offer.key.strip_prefix("www.").unwrap_or(&offer.key)));
            key.add_css_class("offer-key");
            key.set_ellipsize(pango::EllipsizeMode::End);
            key.set_xalign(0.0);
            row.append(&key);
            if !offer.title.is_empty() {
                let title = gtk::Label::new(Some(&offer.title));
                title.add_css_class("offer-title");
                title.set_ellipsize(pango::EllipsizeMode::End);
                title.set_xalign(0.0);
                title.set_hexpand(true);
                row.append(&title);
            }
            let click = gtk::GestureClick::new();
            let me = self.b.clone();
            let offer = offer.clone();
            click.connect_released(move |_, _, _, _| {
                if let Some(b) = me.upgrade() {
                    b.ui().field.take(&offer);
                }
            });
            row.add_controller(click);
            if arrive {
                row.set_opacity(0.0);
                let delay = (i as u64).min(7) * 40;
                let moving = row.clone();
                glib::timeout_add_local_once(Duration::from_millis(delay), move || {
                    if moving.parent().is_none() {
                        return;
                    }
                    motion::animate(&moving, 0.0, 1.0, Curve::Settle, {
                        let moving = moving.clone();
                        move |t| {
                            moving.set_opacity(t);
                            moving.set_margin_top(((1.0 - t) * 8.0).round() as i32);
                        }
                    });
                });
            }
            self.list.append(&row);
        }
        if offers.is_empty() {
            self.list_presence.show(false);
        } else {
            self.list_presence.reveal();
        }
    }

    /// 560 px at rest. Typing widens it to the page column, 12 px clear of each side.
    fn fit(&self) {
        let room = self.column.parent().map(|p| p.width()).unwrap_or(0);
        let wide = !self.typed.borrow().trim().is_empty();
        // Before the column has a width, stay at the rest size. Once it has
        // one, the pill is the column minus 12 px on each side, and never wider.
        let inset = (room - 24).max(1);
        let want = if room < 80 {
            560
        } else if wide {
            inset
        } else {
            560.min(inset)
        };
        self.width.to(want as f64, Curve::Settle);
    }

    /// A row clicked, taken directly.
    fn take(&self, offer: &Offer) {
        let Some(b) = self.browser() else { return };
        self.summoning.set(false);
        match &offer.kind {
            Kind::Command(action) => {
                ActionGroupExt::activate_action(&b.window, action.trim_start_matches("win."), None);
            }
            Kind::Open(id) => {
                if let Some(t) = b.tab(*id) {
                    b.select(&t);
                }
            }
            _ => b.go(&offer.url),
        }
        self.editing.set(false);
        self.typed.borrow_mut().clear();
    }

    /// Return. A row picked from the list wins; then what the field was
    /// finishing for you; then what you typed. `aside`: Ctrl+Return, into a
    /// new tab, behind unless `front`.
    pub fn submit(&self, aside: bool, front: bool) {
        let Some(b) = self.browser() else { return };
        let offers = self.offers.borrow().clone();
        let picked = self.picked.get().and_then(|i| offers.get(i).cloned());
        if let Some(Offer { kind: Kind::Open(id), .. }) = &picked {
            if let Some(t) = b.tab(*id) {
                self.summoning.set(false);
                self.editing.set(false);
                b.select(&t);
            }
            return;
        }
        if self.summoning.replace(false) && self.typed().trim().is_empty() {
            self.dismiss();
            return;
        }
        if let Some(Offer { kind: Kind::Command(action), .. }) = picked.clone().or_else(|| offers.first().cloned()) {
            self.editing.set(false);
            self.typed.borrow_mut().clear();
            self.dismiss();
            ActionGroupExt::activate_action(&b.window, action.trim_start_matches("win."), None);
            return;
        }
        let text = self.text.text().to_string();
        // Finished from history: the page as it was visited, not rebuilt.
        let visited = offers
            .iter()
            .find(|o| o.kind == Kind::Place && crate::history::identity(&o.url) == crate::history::identity(&text))
            .map(|o| o.url.clone());
        let target = match picked {
            Some(o) => Some(o.url),
            None => visited.or_else(|| b.prefs.borrow().destination(&text)),
        };
        let Some(url) = target else {
            self.frame.add_css_class("refused");
            motion::shake(&self.shaker);
            return;
        };
        self.editing.set(false);
        self.typed.borrow_mut().clear();
        if aside {
            let from = b.active();
            b.open(&url, front, from.as_ref());
            self.dismiss();
        } else {
            b.go(&url);
        }
    }
}
