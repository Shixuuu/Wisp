//! What rises from the bottom edge to say one thing, the address of the link
//! under the pointer, and the find field at the top of the page.

use crate::browser::Browser;
use crate::motion::Presence;
use adw::prelude::*;
use gtk::{gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;
use webkit6::prelude::*;

pub struct Bars {
    b: Weak<Browser>,
    said: gtk::Label,
    finder: gtk::Button,
    shown_file: RefCell<Option<String>>,
    announcement: Presence,
    hush: RefCell<Option<glib::SourceId>>,
    ask_text: gtk::Label,
    ask_icon: gtk::Image,
    asked: RefCell<Option<Answer>>,
    asking: Presence,
    hint: Presence,
    bubble: gtk::Label,
    bubble_presence: Presence,
    bubble_hide: RefCell<Option<glib::SourceId>>,
    pointer: Rc<Cell<(f64, f64)>>,
    find: Presence,
    find_frame: gtk::Box,
    pub needle: gtk::Text,
    find_status: gtk::Label,
    match_case: Cell<bool>,
    found: Cell<(u32, u32)>,
}

/// What to do with a page's question once it is answered.
type Answer = Box<dyn Fn(bool)>;

fn capsule(class: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class("capsule");
    b.add_css_class(class);
    b
}

impl Bars {
    pub fn new(b: &Rc<Browser>) -> Rc<Bars> {
        // The line that says one thing.
        let said = gtk::Label::new(None);
        said.add_css_class("said");
        let finder = gtk::Button::with_label("Show in Folder");
        finder.add_css_class("said-link");
        let line = capsule("announcement");
        line.append(&said);
        line.append(&finder);
        let announcement = Presence::new(&line, 1.0, 20.0);

        // A page asking to see, hear or locate you.
        let ask_icon = gtk::Image::new();
        ask_icon.set_pixel_size(12);
        ask_icon.add_css_class("muted");
        let ask_text = gtk::Label::new(None);
        ask_text.add_css_class("ask-text");
        let allow = gtk::Button::with_label("Allow");
        allow.add_css_class("ink-pill");
        let deny = gtk::Button::with_label("Don't allow");
        deny.add_css_class("said-link");
        let ask = capsule("ask");
        ask.append(&ask_icon);
        ask.append(&ask_text);
        ask.append(&allow);
        ask.append(&deny);
        let asking = Presence::new(&ask, 1.0, 20.0);

        // The one mode this browser has, said for as long as it lasts.
        let hint_text = gtk::Label::new(Some("Click anything to hide it   Ctrl+Z undo   esc done"));
        let hint_box = capsule("hint");
        hint_box.append(&hint_text);
        let hint = Presence::new(&hint_box, 1.0, 20.0);

        let stack = gtk::Box::new(gtk::Orientation::Vertical, 8);
        stack.set_valign(gtk::Align::End);
        stack.set_halign(gtk::Align::Center);
        stack.set_margin_bottom(30);
        for p in [&asking, &hint, &announcement] {
            p.root.set_halign(gtk::Align::Center);
            stack.append(&p.root);
        }
        b.root.add_overlay(&stack);

        // The address of the link under the pointer.
        let bubble = gtk::Label::new(None);
        bubble.add_css_class("capsule");
        bubble.add_css_class("bubble");
        bubble.set_ellipsize(pango::EllipsizeMode::Middle);
        bubble.set_max_width_chars(80);
        let bubble_presence = Presence::new(&bubble, 1.0, 0.0);
        bubble_presence.root.set_valign(gtk::Align::End);
        bubble_presence.root.set_halign(gtk::Align::Start);
        bubble_presence.root.set_margin_start(10);
        bubble_presence.root.set_margin_end(10);
        bubble_presence.root.set_margin_bottom(10);
        bubble_presence.root.set_can_target(false);
        b.page.add_overlay(&bubble_presence.root);
        let pointer = Rc::new(Cell::new((0.0, 0.0)));
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        let p = pointer.clone();
        motion.connect_motion(move |_, x, y| p.set((x, y)));
        b.page.add_controller(motion);

        // Find on page.
        let needle = crate::text_box("Find on page");
        needle.set_placeholder_text(Some("Find on page"));
        needle.set_width_chars(16);
        needle.add_css_class("needle");
        let find_status = gtk::Label::new(None);
        find_status.add_css_class("find-status");
        let up = step("go-up-symbolic", "Previous match");
        let down = step("go-down-symbolic", "Next match");
        let options = gtk::MenuButton::new();
        options.set_icon_name("view-more-symbolic");
        options.add_css_class("find-step");
        options.set_tooltip_text(Some("Search options"));
        let menu = gio::Menu::new();
        menu.append(Some("Match case"), Some("win.find-case"));
        options.set_menu_model(Some(&menu));
        let close = step("window-close-symbolic", "Close Find on Page");
        let find_frame = capsule("find");
        for w in [
            needle.upcast_ref::<gtk::Widget>(),
            find_status.upcast_ref(),
            up.upcast_ref(),
            down.upcast_ref(),
            options.upcast_ref(),
            close.upcast_ref(),
        ] {
            find_frame.append(w);
        }
        let find = Presence::new(&find_frame, 1.0, -12.0);
        find.root.set_halign(gtk::Align::End);
        find.root.set_valign(gtk::Align::Start);
        find.root.set_margin_top(12);
        find.root.set_margin_end(14);
        b.page.add_overlay(&find.root);

        let bars = Rc::new(Bars {
            b: b.weak(),
            said,
            finder,
            shown_file: RefCell::default(),
            announcement,
            hush: RefCell::default(),
            ask_text,
            ask_icon,
            asked: RefCell::default(),
            asking,
            hint,
            bubble,
            bubble_presence,
            bubble_hide: RefCell::default(),
            pointer,
            find,
            find_frame,
            needle,
            find_status,
            match_case: Cell::new(false),
            found: Cell::new((0, 0)),
        });

        let me = Rc::downgrade(&bars);
        bars.finder.connect_clicked(move |_| {
            if let Some(path) = me.upgrade().and_then(|m| m.shown_file.borrow().clone()) {
                let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
                launcher.open_containing_folder(None::<&gtk::Window>, None::<&gio::Cancellable>, |_| {});
            }
        });
        for (button, answer) in [(allow, true), (deny, false)] {
            let me = Rc::downgrade(&bars);
            button.connect_clicked(move |_| {
                let Some(me) = me.upgrade() else { return };
                if let Some(asked) = me.asked.take() {
                    asked(answer);
                }
                me.asking.show(false);
            });
        }
        let me = Rc::downgrade(&bars);
        bars.needle.connect_changed(move |_| {
            if let Some(me) = me.upgrade() {
                me.search();
            }
        });
        let me = Rc::downgrade(&bars);
        bars.needle.connect_activate(move |_| {
            if let Some(me) = me.upgrade() {
                me.look(true);
            }
        });
        // Ctrl+Shift+G now groups the active tab. Shift+Enter still walks
        // back through the matches.
        let previous = gtk::EventControllerKey::new();
        previous.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = Rc::downgrade(&bars);
        previous.connect_key_pressed(move |_, key, _, mods| {
            if matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
                && mods.contains(gtk::gdk::ModifierType::SHIFT_MASK)
            {
                if let Some(me) = me.upgrade() {
                    me.look(false);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        bars.needle.add_controller(previous);
        let me = Rc::downgrade(&bars);
        up.connect_clicked(move |_| {
            if let Some(me) = me.upgrade() {
                me.look(false);
            }
        });
        let me = Rc::downgrade(&bars);
        down.connect_clicked(move |_| {
            if let Some(me) = me.upgrade() {
                me.look(true);
            }
        });
        let weak = b.weak();
        close.connect_clicked(move |_| {
            if let Some(b) = weak.upgrade() {
                b.ui().bars.close_find(&b);
            }
        });
        let case = gio::SimpleAction::new_stateful("find-case", None, &false.to_variant());
        let me = Rc::downgrade(&bars);
        case.connect_activate(move |a, _| {
            let on = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            a.set_state(&on.to_variant());
            if let Some(me) = me.upgrade() {
                me.match_case.set(on);
                me.search();
            }
        });
        b.window.add_action(&case);
        bars
    }

    /// A line that rises, says one thing, and leaves: 1.7 s, or 4 s with a
    /// file to show.
    pub fn announce(&self, text: &str, file: Option<String>) {
        self.said.set_label(text);
        self.finder.set_visible(file.is_some());
        let long = file.is_some();
        *self.shown_file.borrow_mut() = file;
        self.announcement.show(true);
        if let Some(id) = self.hush.take() {
            id.remove();
        }
        let presence = self.announcement.clone();
        let me = self.b.clone();
        *self.hush.borrow_mut() =
            Some(glib::timeout_add_local_once(Duration::from_millis(if long { 4000 } else { 1700 }), move || {
                presence.show(false);
                if let Some(b) = me.upgrade() {
                    b.ui().bars.hush.take();
                }
            }));
    }

    pub fn ask(&self, kind: &str, text: &str, answer: impl Fn(bool) + 'static) {
        if let Some(earlier) = self.asked.take() {
            earlier(false);
        }
        self.ask_icon.set_icon_name(Some(match kind {
            "location" => "find-location-symbolic",
            "microphone" => "audio-input-microphone-symbolic",
            "notifications" => "preferences-system-notifications-symbolic",
            _ => "camera-web-symbolic",
        }));
        self.ask_text.set_label(text);
        *self.asked.borrow_mut() = Some(Box::new(answer));
        self.asking.show(true);
    }

    pub fn hint(&self, on: bool) {
        self.hint.show(on);
    }

    /// The link under the pointer, bottom left of the page; bottom right
    /// when the pointer is where the bubble would be.
    pub fn link(&self, address: Option<String>) {
        if let Some(id) = self.bubble_hide.take() {
            id.remove();
        }
        match address {
            Some(a) => {
                self.bubble.set_label(&a);
                let Some(b) = self.b.upgrade() else { return };
                let (x, y) = self.pointer.get();
                let (w, h) = (b.page.width() as f64, b.page.height() as f64);
                let right = h - y < 50.0 && x < (w * 0.6).min(640.0) + 22.0;
                self.bubble_presence.root.set_halign(if right { gtk::Align::End } else { gtk::Align::Start });
                self.bubble_presence.show(true);
            }
            None => {
                let p = self.bubble_presence.clone();
                let me = self.b.clone();
                *self.bubble_hide.borrow_mut() =
                    Some(glib::timeout_add_local_once(Duration::from_millis(120), move || {
                        p.show(false);
                        if let Some(b) = me.upgrade() {
                            b.ui().bars.bubble_hide.take();
                        }
                    }));
            }
        }
    }

    // MARK: find

    pub fn finding(&self) -> bool {
        self.find.shown()
    }

    pub fn open_find(&self) {
        self.find.show(true);
        self.needle.grab_focus();
        self.needle.select_region(0, -1);
        self.search();
    }

    pub fn close_find(&self, b: &Browser) {
        if !self.find.shown() {
            return;
        }
        self.find.show(false);
        if let Some(view) = b.active().and_then(|t| t.view.borrow().clone()) {
            if let Some(f) = view.find_controller() {
                f.search_finish();
            }
            view.grab_focus();
        }
    }

    fn controller(&self) -> Option<webkit6::FindController> {
        let b = self.b.upgrade()?;
        let view = b.active()?.view.borrow().clone()?;
        let finder = view.find_controller()?;
        if unsafe { finder.data::<bool>("wisp") }.is_none() {
            unsafe { finder.set_data("wisp", true) };
            let me = self.b.clone();
            finder.connect_counted_matches(move |_, n| {
                if let Some(b) = me.upgrade() {
                    let bars = &b.ui().bars;
                    bars.found.set((if n > 0 { 1 } else { 0 }, n));
                    bars.show_status();
                }
            });
            let me = self.b.clone();
            finder.connect_failed_to_find_text(move |_| {
                if let Some(b) = me.upgrade() {
                    let bars = &b.ui().bars;
                    bars.found.set((0, 0));
                    bars.show_status();
                }
            });
        }
        Some(finder)
    }

    fn options(&self) -> u32 {
        let mut o = webkit6::FindOptions::WRAP_AROUND;
        if !self.match_case.get() {
            o |= webkit6::FindOptions::CASE_INSENSITIVE;
        }
        o.bits()
    }

    fn search(&self) {
        let Some(finder) = self.controller() else { return };
        let text = self.needle.text().to_string();
        if text.is_empty() {
            finder.search_finish();
            self.found.set((0, 0));
            self.find_status.set_label("");
            self.find_frame.remove_css_class("missed");
            return;
        }
        finder.count_matches(&text, self.options(), 1000);
        finder.search(&text, self.options(), 1000);
    }

    /// Ctrl+G and Ctrl+Shift+G: the next match, or the one before.
    pub fn look(&self, forward: bool) {
        let Some(finder) = self.controller() else { return };
        if self.needle.text().is_empty() {
            return;
        }
        let (at, total) = self.found.get();
        if total > 0 {
            let next = if forward {
                at % total + 1
            } else if at <= 1 {
                total
            } else {
                at - 1
            };
            self.found.set((next, total));
        }
        if forward {
            finder.search_next()
        } else {
            finder.search_previous()
        }
        self.show_status();
    }

    fn show_status(&self) {
        let (at, total) = self.found.get();
        let missed = total == 0 && !self.needle.text().is_empty();
        self.find_status.set_label(&if missed {
            "Not found".into()
        } else if total > 0 {
            format!("{at} of {total}")
        } else {
            String::new()
        });
        if missed { self.find_frame.add_css_class("missed") } else { self.find_frame.remove_css_class("missed") }
    }
}

fn step(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("find-step");
    b.set_tooltip_text(Some(tip));
    b
}
