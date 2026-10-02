//! Ctrl+Tab: the tabs you used most recently, as small pictures. Held, it
//! comes up after a moment; let go of Ctrl and you are on the one chosen.
//! A quick Ctrl+Tab without waiting just goes back to the last tab.

use crate::browser::Browser;
use crate::motion::{Curve, Presence, Tween};
use crate::tab::Tab;
use adw::prelude::*;
use gtk::{glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

const COLUMNS: usize = 5;
const MOST: usize = 10;

pub struct Switcher {
    b: Weak<Browser>,
    recent: RefCell<Vec<u64>>,
    candidates: RefCell<Vec<u64>>,
    selected: Cell<usize>,
    layer: crate::motion::Place,
    panel: Presence,
    grid: gtk::Fixed,
    ground: gtk::Box,
    gx: Tween,
    gy: Tween,
    reveal: RefCell<Option<glib::SourceId>>,
    hide_later: Rc<RefCell<Option<glib::SourceId>>>,
    card: Cell<(f64, f64)>,
}

impl Switcher {
    pub fn new(b: &Rc<Browser>) -> Rc<Switcher> {
        let dim = gtk::Box::new(gtk::Orientation::Vertical, 0);
        dim.add_css_class("dim-switcher");
        let ground = gtk::Box::new(gtk::Orientation::Vertical, 0);
        ground.add_css_class("switch-ground");
        let grid = gtk::Fixed::new();
        grid.put(&ground, 0.0, 0.0);
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("switcher");
        frame.append(&grid);
        let panel = Presence::new(&frame, 0.97, 0.0);
        panel.root.set_halign(gtk::Align::Center);
        panel.root.set_valign(gtk::Align::Center);
        let layer = gtk::Overlay::new();
        layer.set_child(Some(&dim));
        layer.add_overlay(&panel.root);
        layer.set_vexpand(true);
        let layer = crate::motion::Place::new(&layer);
        b.root.add_overlay(&layer.root);
        let (gx, gy) = crate::tabs::axes(&grid, &ground);
        let s = Rc::new(Switcher {
            b: b.weak(),
            recent: RefCell::default(),
            candidates: RefCell::default(),
            selected: Cell::new(0),
            layer,
            panel,
            grid,
            ground,
            gx,
            gy,
            reveal: RefCell::default(),
            hide_later: Rc::new(RefCell::default()),
            card: Cell::new((176.0, 140.0)),
        });
        let click = gtk::GestureClick::new();
        let me = Rc::downgrade(&s);
        click.connect_released(move |_, _, _, _| {
            if let Some(me) = me.upgrade() {
                me.cancel();
            }
        });
        dim.add_controller(click);
        s
    }

    /// A tab stepped away from goes to the front of the recent list.
    pub fn left(&self, tab: &Tab) {
        let mut recent = self.recent.borrow_mut();
        recent.retain(|&id| id != tab.id);
        recent.insert(0, tab.id);
        recent.truncate(MOST * 2);
    }

    pub fn active(&self) -> bool {
        !self.candidates.borrow().is_empty()
    }

    /// Ctrl+Tab, or Ctrl+Shift+Tab to go the other way.
    pub fn step(self: &Rc<Self>, backwards: bool) {
        let Some(b) = self.b.upgrade() else { return };
        if self.candidates.borrow().is_empty() {
            let Some(current) = b.active() else { return };
            let tabs = b.tabs.borrow();
            let alive: Vec<u64> = tabs.iter().map(|t| t.id).collect();
            let mut list = vec![current.id];
            for id in self.recent.borrow().iter().chain(alive.iter()) {
                if alive.contains(id) && !list.contains(id) {
                    list.push(*id);
                }
            }
            list.truncate(MOST);
            drop(tabs);
            if list.len() < 2 {
                return;
            }
            let n = list.len();
            *self.candidates.borrow_mut() = list;
            self.selected.set(if backwards { n - 1 } else { 1 });
            let me = Rc::downgrade(self);
            *self.reveal.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_millis(150), move || {
                if let Some(me) = me.upgrade() {
                    me.reveal.take();
                    me.show();
                }
            }));
            return;
        }
        let n = self.candidates.borrow().len();
        let at = self.selected.get();
        self.selected.set(if backwards { (at + n - 1) % n } else { (at + 1) % n });
        self.show();
        self.place_ground(true);
    }

    /// The arrow keys, while it is up.
    pub fn nudge(self: &Rc<Self>, dx: isize, dy: isize) {
        let n = self.candidates.borrow().len() as isize;
        if n == 0 {
            return;
        }
        let at = self.selected.get() as isize;
        let next = if dx != 0 {
            (at + dx + n) % n
        } else {
            let next = at + dy * COLUMNS as isize;
            if !(0..n).contains(&next) {
                return;
            }
            next
        };
        self.selected.set(next as usize);
        self.show();
        self.place_ground(true);
    }

    /// Ctrl let go of: the chosen tab.
    pub fn commit(self: &Rc<Self>) {
        let chosen = self.candidates.borrow().get(self.selected.get()).copied();
        self.cancel();
        let Some(b) = self.b.upgrade() else { return };
        if let Some(tab) = chosen.and_then(|id| b.tab(id)) {
            b.select(&tab);
        }
    }

    pub fn cancel(&self) {
        if let Some(id) = self.reveal.take() {
            id.remove();
        }
        if let Some(id) = self.hide_later.take() {
            id.remove();
        }
        self.candidates.borrow_mut().clear();
        self.panel.show(false);
        let layer = self.layer.clone();
        let later = Rc::clone(&self.hide_later);
        *self.hide_later.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_millis(160), move || {
            later.take();
            layer.show(false);
        }));
    }

    fn show(self: &Rc<Self>) {
        if let Some(id) = self.hide_later.take() {
            id.remove();
        }
        if self.panel.shown() {
            return;
        }
        if let Some(id) = self.reveal.take() {
            id.remove();
        }
        let Some(b) = self.b.upgrade() else { return };
        if let Some(active) = b.active() {
            b.capture(&active);
        }
        self.build(&b);
        self.layer.show(true);
        self.panel.show(true);
        self.place_ground(false);
    }

    fn build(self: &Rc<Self>, b: &Rc<Browser>) {
        let mut child = self.grid.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if c != self.ground.clone().upcast::<gtk::Widget>() {
                self.grid.remove(&c);
            }
        }
        let ids = self.candidates.borrow().clone();
        let columns = ids.len().min(COLUMNS);
        let room = b.window.width() as f64 - 64.0;
        let width = (176.0_f64).min((room - (columns as f64 - 1.0) * 8.0) / columns as f64);
        let picture = (width - 16.0) * 0.62;
        let height = picture + 39.0;
        self.card.set((width, height));
        self.ground.set_size_request(width as i32, height as i32);
        for (i, id) in ids.iter().enumerate() {
            let Some(tab) = b.tab(*id) else { continue };
            let card = self.card_for(&tab, width, picture, i);
            let (col, row) = (i % COLUMNS, i / COLUMNS);
            self.grid.put(&card, col as f64 * (width + 8.0), row as f64 * (height + 8.0));
        }
        let rows = ids.len().div_ceil(COLUMNS);
        self.grid.set_size_request(
            (columns as f64 * (width + 8.0) - 8.0) as i32,
            (rows as f64 * (height + 8.0) - 8.0) as i32,
        );
    }

    fn card_for(self: &Rc<Self>, tab: &Rc<Tab>, width: f64, picture: f64, index: usize) -> gtk::Widget {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 7);
        card.add_css_class("switch-card");
        card.set_size_request(width as i32, -1);
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("switch-picture");
        frame.set_size_request((width - 16.0) as i32, picture as i32);
        frame.set_overflow(gtk::Overflow::Hidden);
        match tab.preview.borrow().as_ref() {
            Some(texture) => {
                let p = gtk::Picture::for_paintable(texture);
                p.set_content_fit(gtk::ContentFit::Cover);
                // A picture asks to be as big as the page it shows; this
                // keeps it to the card, however large that is.
                let window = gtk::ScrolledWindow::new();
                window.set_policy(gtk::PolicyType::External, gtk::PolicyType::External);
                window.set_min_content_width((width - 16.0) as i32);
                window.set_min_content_height(picture as i32);
                window.set_child(Some(&p));
                window.set_can_target(false);
                frame.append(&window);
            }
            None => {
                let m = gtk::Label::new(Some(&tab.monogram()));
                m.add_css_class("mark");
                m.add_css_class("big");
                m.set_vexpand(true);
                m.set_halign(gtk::Align::Center);
                m.set_valign(gtk::Align::Center);
                frame.append(&m);
            }
        }
        let caption = gtk::Label::new(Some(&tab.label()));
        caption.add_css_class("switch-caption");
        caption.set_ellipsize(pango::EllipsizeMode::End);
        caption.set_xalign(0.0);
        card.append(&frame);
        card.append(&caption);
        let button = gtk::Button::new();
        button.add_css_class("switch-button");
        button.set_child(Some(&card));
        let me = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(me) = me.upgrade() {
                me.selected.set(index);
                me.commit();
            }
        });
        let hover = gtk::EventControllerMotion::new();
        let me = Rc::downgrade(self);
        hover.connect_enter(move |_, _, _| {
            if let Some(me) = me.upgrade() {
                me.selected.set(index);
                me.place_ground(true);
            }
        });
        button.add_controller(hover);
        button.upcast()
    }

    /// The grey behind the chosen card glides to it.
    fn place_ground(&self, animated: bool) {
        let (w, h) = self.card.get();
        let at = self.selected.get();
        let (x, y) = ((at % COLUMNS) as f64 * (w + 8.0), (at / COLUMNS) as f64 * (h + 8.0));
        if animated {
            self.gx.to(x, Curve::Glide);
            self.gy.to(y, Curve::Glide);
        } else {
            self.gx.set(x);
            self.gy.set(y);
        }
    }
}
