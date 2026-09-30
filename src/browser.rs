//! The window and everything it holds: the tabs, the one that is showing,
//! and the pieces drawn around the page.

use crate::bars::Bars;
use crate::bookmarks::Bookmarks;
use crate::curtain::Curtain;
use crate::history::History;
use crate::loot::{Keep, Loot};
use crate::motion::{Curve, Slide, Tween};
use crate::omnibox::Omnibox;
use crate::panels::Panels;
use crate::settings::{Look, Prefs};
use crate::store;
use crate::switcher::Switcher;
use crate::tab::{Change, Tab};
use crate::tabs::TabList;
use crate::web::Web;
use adw::prelude::*;
use gtk::glib;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;
use webkit6::prelude::*;

/// The strip across the top, when there is one.
pub const STRIP: f64 = 52.0;

/// A tab that was closed, for Ctrl+Shift+T.
struct Ghost {
    url: String,
    title: String,
    index: usize,
}

/// A download on its way.
pub struct Fetch {
    pub download: webkit6::Download,
    pub name: String,
    pub path: String,
    pub from: String,
    pub failed: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    tabs: Vec<SavedTab>,
    active: usize,
}

#[derive(Serialize, Deserialize)]
struct SavedTab {
    url: String,
    title: String,
    #[serde(default)]
    pin: Option<String>,
    #[serde(default)]
    home: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

pub struct Ui {
    pub tabs: RefCell<Option<TabList>>,
    pub field: Rc<Omnibox>,
    pub bars: Rc<Bars>,
    pub panels: Rc<Panels>,
    pub switcher: Rc<Switcher>,
}

pub struct Browser {
    pub window: adw::ApplicationWindow,
    pub prefs: RefCell<Prefs>,
    pub history: RefCell<History>,
    pub bookmarks: RefCell<Bookmarks>,
    pub curtain: RefCell<Curtain>,
    pub loot: RefCell<Loot>,
    pub web: Rc<Web>,
    pub tabs: RefCell<Vec<Rc<Tab>>>,
    active: Cell<Option<u64>>,
    ghosts: RefCell<Vec<Ghost>>,
    next: Cell<u64>,
    pub fetches: RefCell<Vec<Fetch>>,
    pub folded: Cell<bool>,
    pub peeking: Cell<bool>,
    pub veiling: Cell<bool>,
    immersed: Cell<bool>,
    saving: Cell<bool>,
    /// The root of the window: the frame, and everything floating over it.
    pub root: gtk::Overlay,
    /// The page, and what floats over the page alone.
    pub page: gtk::Overlay,
    pub stage: gtk::Stack,
    trouble: crate::motion::Place,
    trouble_text: gtk::Label,
    page_holder: gtk::Box,
    chrome: Slide,
    slide: RefCell<Option<Tween>>,
    room_ticket: Cell<u32>,
    pub ui: OnceCell<Ui>,
}

impl Browser {
    pub fn new(app: &adw::Application) -> Rc<Browser> {
        let prefs = Prefs::load();
        let web = Rc::new(Web::new(&prefs));
        apply_look(prefs.look);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Wisp")
            .default_width(1280)
            .default_height(820)
            .build();
        window.add_css_class("wisp");

        let stage = gtk::Stack::new();
        stage.set_hexpand(true);
        stage.set_vexpand(true);
        let blank = gtk::Box::new(gtk::Orientation::Vertical, 0);
        blank.add_css_class("ground");
        stage.add_named(&blank, Some("blank"));

        let (trouble, trouble_text, retry) = trouble_page();
        let page = gtk::Overlay::new();
        page.set_child(Some(&stage));
        let trouble = crate::motion::Place::new(&trouble);
        page.add_overlay(&trouble.root);

        let page_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page_holder.append(&page);
        page_holder.set_hexpand(true);
        page_holder.set_vexpand(true);

        let chrome_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let chrome = Slide::new(&chrome_box);
        let frame = gtk::Overlay::new();
        frame.set_child(Some(&page_holder));
        frame.add_overlay(&chrome);
        let root = gtk::Overlay::new();
        root.set_child(Some(&frame));
        window.set_content(Some(&root));

        let b = Rc::new(Browser {
            window,
            prefs: RefCell::new(prefs),
            history: RefCell::new(History::load()),
            bookmarks: RefCell::new(Bookmarks::load()),
            curtain: RefCell::new(Curtain::load()),
            loot: RefCell::new(Loot::load()),
            web,
            tabs: RefCell::default(),
            active: Cell::new(None),
            ghosts: RefCell::default(),
            next: Cell::new(1),
            fetches: RefCell::default(),
            folded: Cell::new(false),
            peeking: Cell::new(false),
            veiling: Cell::new(false),
            immersed: Cell::new(false),
            saving: Cell::new(false),
            root,
            page,
            stage,
            trouble,
            trouble_text,
            page_holder,
            chrome,
            slide: RefCell::default(),
            room_ticket: Cell::new(0),
            ui: OnceCell::new(),
        });

        let _ = b.ui.set(Ui {
            tabs: RefCell::new(None),
            field: Omnibox::new(&b),
            bars: Bars::new(&b),
            panels: Panels::new(&b),
            switcher: Switcher::new(&b),
        });
        let weak = b.weak();
        retry.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade().and_then(|b| b.active()).and_then(|t| t.view.borrow().clone()) {
                v.reload();
            }
        });
        b.folded.set(b.prefs.borrow().sidebar && b.prefs.borrow().side_hides);
        b.build_chrome();
        crate::keys::install(&b);
        b.watch_downloads(&b.web.session.clone());
        b.start_shield();
        b.start_timers();
        b.watch_edge();
        b.restore();
        let weak = b.weak();
        b.window.connect_close_request(move |_| {
            if let Some(b) = weak.upgrade() {
                b.save_now();
                b.history.borrow_mut().save();
                b.prefs.borrow().save();
            }
            glib::Propagation::Proceed
        });
        b
    }

    pub fn weak(self: &Rc<Self>) -> Weak<Self> {
        Rc::downgrade(self)
    }

    pub fn ui(&self) -> &Ui {
        self.ui.get().expect("ui is built with the browser")
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn tab(&self, id: u64) -> Option<Rc<Tab>> {
        self.tabs.borrow().iter().find(|t| t.id == id).cloned()
    }

    pub fn active(&self) -> Option<Rc<Tab>> {
        self.active.get().and_then(|id| self.tab(id))
    }

    pub fn is_active(&self, tab: &Tab) -> bool {
        self.active.get() == Some(tab.id)
    }

    pub fn announce(&self, text: &str) {
        self.ui().bars.announce(text, None);
    }

    // MARK: the chrome and the room it takes

    /// The column or the strip, built for whichever the settings ask for.
    pub fn build_chrome(self: &Rc<Self>) {
        let list = TabList::new(self);
        let holder = self.chrome.child().and_downcast::<gtk::Box>().expect("chrome holds a box");
        while let Some(c) = holder.first_child() {
            holder.remove(&c);
        }
        holder.append(&list.root);
        let (side, right, width) = {
            let p = self.prefs.borrow();
            (p.sidebar, p.side_right, p.side_width)
        };
        if side {
            self.chrome.set_halign(if right { gtk::Align::End } else { gtk::Align::Start });
            self.chrome.set_valign(gtk::Align::Fill);
            list.root.set_size_request(width as i32, -1);
        } else {
            self.chrome.set_halign(gtk::Align::Fill);
            self.chrome.set_valign(gtk::Align::Start);
        }
        *self.ui().tabs.borrow_mut() = Some(list);
        let chrome = self.chrome.clone();
        let weak = self.weak();
        let tween = Tween::new(&self.chrome, 0.0, move |v| {
            let Some(b) = weak.upgrade() else { return };
            let p = b.prefs.borrow();
            let (dx, dy) = if !p.sidebar {
                (0.0, -v * STRIP)
            } else if p.side_right {
                (v * p.side_width, 0.0)
            } else {
                (-v * p.side_width, 0.0)
            };
            chrome.set_offset(dx, dy);
            chrome.set_can_target(v < 0.99);
        });
        *self.slide.borrow_mut() = Some(tween);
        self.arrange(false);
        self.refresh_tabs();
    }

    fn tween(&self) -> Tween {
        self.slide.borrow().clone().expect("chrome has a tween")
    }

    /// The room the column or the strip takes from the page. Chrome going
    /// away gives the page its room at once, and the page slides out from
    /// under it; chrome arriving slides over the page, which gives up its
    /// room once the slide is over (Search's `make(room:)`).
    pub fn arrange(self: &Rc<Self>, animated: bool) {
        let shown = !self.folded.get() && !self.immersed.get();
        let (side, right, width) = {
            let p = self.prefs.borrow();
            (p.sidebar, p.side_right, p.side_width)
        };
        let extent = if side { width } else { STRIP };
        let over = self.peeking.get() && !shown;
        let tween = self.tween();
        let target = if shown || over { 0.0 } else { 1.0 };
        let was_hidden = tween.target() > 0.5;
        if animated {
            tween.to(target, Curve::Glide);
        } else {
            tween.set(target);
        }
        if over {
            self.chrome.add_css_class("peek");
        } else {
            self.chrome.remove_css_class("peek");
        }
        let set_room = {
            let holder = self.page_holder.clone();
            move |room: f64| {
                let r = room as i32;
                holder.set_margin_start(if side && !right { r } else { 0 });
                holder.set_margin_end(if side && right { r } else { 0 });
                holder.set_margin_top(if side { 0 } else { r });
            }
        };
        let ticket = self.room_ticket.get() + 1;
        self.room_ticket.set(ticket);
        if !shown {
            set_room(0.0);
        } else if animated && was_hidden {
            let weak = self.weak();
            glib::timeout_add_local_once(Duration::from_millis(420), move || {
                if let Some(b) = weak.upgrade()
                    && b.room_ticket.get() == ticket
                {
                    set_room(extent);
                }
            });
        } else {
            set_room(extent);
        }
    }

    /// Ctrl+S: the column or the strip folded away, and slid out over the
    /// page for a look when the pointer reaches the edge.
    pub fn toggle_fold(self: &Rc<Self>) {
        self.peeking.set(false);
        self.folded.set(!self.folded.get());
        self.arrange(true);
    }

    pub fn peek(self: &Rc<Self>, out: bool) {
        if self.peeking.get() == out || !self.folded.get() {
            return;
        }
        self.peeking.set(out);
        self.arrange(true);
    }

    /// Ctrl+Shift+S: the same tabs, down the side or across the top.
    pub fn toggle_sidebar(self: &Rc<Self>) {
        let side = !self.prefs.borrow().sidebar;
        self.prefs.borrow_mut().sidebar = side;
        self.prefs.borrow().save();
        self.folded.set(side && self.prefs.borrow().side_hides);
        self.peeking.set(false);
        self.build_chrome();
        // In from its own edge.
        let tween = self.tween();
        tween.set(1.0);
        tween.to(0.0, Curve::Glide);
    }

    pub fn set_side_width(self: &Rc<Self>, width: f64) {
        let width = width.clamp(crate::settings::SIDE_MIN, crate::settings::SIDE_MAX);
        self.prefs.borrow_mut().side_width = width;
        if let Some(list) = self.ui().tabs.borrow().as_ref() {
            list.root.set_size_request(width as i32, -1);
            list.relayout();
        }
        self.arrange(false);
    }

    /// Pushing the pointer against the edge of a folded window brings the
    /// tabs out after a moment; leaving them puts them away a moment later.
    fn watch_edge(self: &Rc<Self>) {
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        let arriving: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        let leaving: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
        let weak = self.weak();
        motion.connect_motion(move |_, x, y| {
            let Some(b) = weak.upgrade() else { return };
            if !b.folded.get() || b.immersed.get() {
                return;
            }
            let (side, right, width) = {
                let p = b.prefs.borrow();
                (p.sidebar, p.side_right, p.side_width)
            };
            let w = b.root.width() as f64;
            let distance = if !side {
                y
            } else if right {
                w - x
            } else {
                x
            };
            let reach = if side { width } else { STRIP };
            let cancel = |slot: &Rc<RefCell<Option<glib::SourceId>>>| {
                if let Some(id) = slot.take() {
                    id.remove();
                }
            };
            if b.peeking.get() {
                if distance < reach {
                    cancel(&leaving);
                } else if leaving.borrow().is_none() {
                    let weak = b.weak();
                    let slot = leaving.clone();
                    *leaving.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_millis(300), move || {
                        slot.take();
                        if let Some(b) = weak.upgrade() {
                            b.peek(false);
                        }
                    }));
                }
            } else if distance < 6.0 {
                if arriving.borrow().is_none() {
                    let weak = b.weak();
                    let slot = arriving.clone();
                    *arriving.borrow_mut() =
                        Some(glib::timeout_add_local_once(Duration::from_millis(150), move || {
                            slot.take();
                            if let Some(b) = weak.upgrade() {
                                b.peek(true);
                            }
                        }));
                }
            } else {
                cancel(&arriving);
            }
        });
        self.root.add_controller(motion);
    }

    /// A page filling the screen takes the chrome with it.
    pub fn immerse(self: &Rc<Self>, on: bool) {
        self.immersed.set(on);
        if on {
            self.window.fullscreen();
        } else {
            self.window.unfullscreen();
        }
        self.arrange(false);
    }

    // MARK: the page

    pub fn stage_add(&self, tab: &Tab) {
        let Some(view) = tab.view.borrow().clone() else { return };
        if view.parent().is_none() {
            self.stage.add_named(&view, Some(&tab.id.to_string()));
        }
    }

    pub fn stage_remove(&self, view: &webkit6::WebView) {
        if view.parent().is_some() {
            self.stage.remove(view);
        }
    }

    fn show_stage(&self) {
        let Some(tab) = self.active() else { return };
        let name = if tab.view.borrow().is_some() { tab.id.to_string() } else { "blank".into() };
        self.stage.set_visible_child_name(&name);
        let failure = tab.failure.borrow().clone();
        self.trouble.show(failure.is_some());
        self.trouble_text.set_label(&failure.unwrap_or_default());
    }

    /// Something about a tab changed: redraw what shows it.
    pub fn changed(self: &Rc<Self>, tab: &Rc<Tab>, what: Change) {
        if let Some(list) = self.ui().tabs.borrow().as_ref() {
            list.update(tab, what);
        }
        if !self.is_active(tab) {
            if matches!(what, Change::Address | Change::Title) {
                self.save_soon();
            }
            return;
        }
        match what {
            Change::Title => {
                self.window.set_title(Some(&tab.label()));
                self.save_soon();
            }
            Change::Address => {
                self.window.set_title(Some(&tab.label()));
                self.save_soon();
            }
            Change::Failure => self.show_stage(),
            _ => {}
        }
    }

    pub fn refresh_tabs(self: &Rc<Self>) {
        if let Some(list) = self.ui().tabs.borrow().as_ref() {
            list.sync();
        }
    }

    pub fn hover_link(&self, link: Option<String>) {
        if self.prefs.borrow().shows_links {
            self.ui().bars.link(link);
        }
    }

    // MARK: tabs

    fn make(&self, url: Option<String>, title: &str, shy: bool) -> Rc<Tab> {
        let id = self.next.get();
        self.next.set(id + 1);
        let tab = Rc::new(Tab::new(id, url, title, shy));
        tab.zoom.set(self.prefs.borrow().page_zoom);
        tab
    }

    /// A new tab, after `after` or at the end.
    pub fn insert(self: &Rc<Self>, url: Option<String>, title: &str, shy: bool, after: Option<&Tab>) -> Rc<Tab> {
        let tab = self.make(url, title, shy);
        {
            let mut tabs = self.tabs.borrow_mut();
            let at = after.and_then(|a| tabs.iter().position(|t| t.id == a.id)).map(|i| i + 1).unwrap_or(tabs.len());
            let pins = tabs.iter().filter(|t| t.pin.borrow().is_some()).count();
            let at = at.max(pins.min(tabs.len()));
            tabs.insert(at, tab.clone());
        }
        self.refresh_tabs();
        self.save_soon();
        tab
    }

    /// Show a tab: its page woken if it slept, brought forward.
    pub fn select(self: &Rc<Self>, tab: &Rc<Tab>) {
        let field = &self.ui().field;
        if let Some(here) = self.active() {
            if here.id == tab.id {
                field.follow(tab);
                self.show_stage();
                return;
            }
            if here.is_blank() {
                *here.draft.borrow_mut() = field.typed();
            }
            self.capture(&here);
            here.touch();
            self.ui().switcher.left(&here);
        }
        if self.veiling.get() {
            self.toggle_hiding();
        }
        self.ui().bars.close_find(self);
        self.active.set(Some(tab.id));
        tab.touch();
        self.wake(tab);
        self.stage_add(tab);
        self.show_stage();
        self.window.set_title(Some(&tab.label()));
        field.follow(tab);
        self.refresh_tabs();
        self.ui().bars.link(None);
        if let Some(view) = tab.view.borrow().as_ref() {
            view.grab_focus();
        }
        self.save_soon();
    }

    pub fn select_index(self: &Rc<Self>, n: usize) {
        let target = {
            let tabs = self.tabs.borrow();
            if n >= 9 { tabs.last().cloned() } else { tabs.get(n - 1).cloned() }
        };
        if let Some(t) = target {
            self.select(&t);
        }
    }

    pub fn step(self: &Rc<Self>, by: isize) {
        let next = {
            let tabs = self.tabs.borrow();
            let n = tabs.len() as isize;
            let Some(i) = self.active.get().and_then(|id| tabs.iter().position(|t| t.id == id)) else { return };
            if n < 2 {
                return;
            }
            tabs[(((i as isize + by) % n + n) % n) as usize].clone()
        };
        self.select(&next);
    }

    /// Ctrl+T. A blank tab already open is reused, not made again.
    pub fn new_tab(self: &Rc<Self>) {
        let shy = self.active().is_some_and(|t| t.shy);
        let blank = self.tabs.borrow().iter().rev().find(|t| t.is_blank() && t.shy == shy).cloned();
        let tab = match blank {
            Some(t) => {
                let last = self.tabs.borrow().len() - 1;
                self.move_tab(&t, last);
                *t.draft.borrow_mut() = String::new();
                t
            }
            None => self.insert(None, "", shy, None),
        };
        self.select(&tab);
        if shy {
            self.announce("A tab that keeps nothing");
        }
    }

    /// Ctrl+Shift+N.
    pub fn new_shy_tab(self: &Rc<Self>) {
        let blank = self.tabs.borrow().iter().rev().find(|t| t.is_blank() && t.shy).cloned();
        let tab = blank.unwrap_or_else(|| self.insert(None, "", true, None));
        self.select(&tab);
        self.announce("A tab that keeps nothing");
    }

    /// An address in a new tab beside `from`. From a private tab, private too.
    pub fn open(self: &Rc<Self>, url: &str, foreground: bool, from: Option<&Rc<Tab>>) -> Rc<Tab> {
        let shy = from.is_some_and(|t| t.shy);
        let tab = self.insert(Some(url.to_string()), "", shy, from.map(|t| t.as_ref()));
        if foreground {
            self.select(&tab);
        } else if !self.prefs.borrow().lazy_tabs {
            let view = self.build(&tab, None);
            view.load_uri(url);
            self.stage_add(&tab);
            self.refresh_tabs();
        }
        tab
    }

    /// Go somewhere in the tab that is showing.
    pub fn go(self: &Rc<Self>, url: &str) {
        let tab = match self.active() {
            Some(t) => t,
            None => self.insert(None, "", false, None),
        };
        *tab.url.borrow_mut() = Some(url.to_string());
        *tab.failure.borrow_mut() = None;
        let existing = tab.view.borrow().clone();
        let view = existing.unwrap_or_else(|| self.build(&tab, None));
        view.load_uri(url);
        self.stage_add(&tab);
        self.select(&tab);
        self.show_stage();
        view.grab_focus();
        self.changed(&tab, Change::Address);
    }

    pub fn move_tab(self: &Rc<Self>, tab: &Tab, to: usize) {
        {
            let mut tabs = self.tabs.borrow_mut();
            let Some(i) = tabs.iter().position(|t| t.id == tab.id) else { return };
            let t = tabs.remove(i);
            let to = to.min(tabs.len());
            tabs.insert(to, t);
        }
        self.refresh_tabs();
        self.save_soon();
    }

    /// Ctrl+W on a tab. A pinned tab is put down, not closed: it sleeps.
    pub fn close(self: &Rc<Self>, tab: &Rc<Tab>) {
        let Some(index) = self.tabs.borrow().iter().position(|t| t.id == tab.id) else { return };
        let was_active = self.is_active(tab);
        if tab.pin.borrow().is_some() {
            self.sleep(tab);
            if was_active {
                let others: Vec<Rc<Tab>> =
                    self.tabs.borrow().iter().filter(|t| t.id != tab.id && !t.asleep()).cloned().collect();
                let loose: Vec<Rc<Tab>> = others.iter().filter(|t| t.pin.borrow().is_none()).cloned().collect();
                let pool = if loose.is_empty() { others } else { loose };
                match pool.iter().max_by_key(|t| t.touched.get()) {
                    Some(back) => self.select(back),
                    None => self.new_tab(),
                }
            }
            self.refresh_tabs();
            self.save_soon();
            return;
        }
        if !tab.is_blank() && !tab.shy {
            self.ghosts.borrow_mut().push(Ghost { url: tab.address(), title: tab.label(), index });
        }
        if self.tabs.borrow().len() == 1 {
            if tab.is_blank() {
                self.window.close();
                return;
            }
            self.sleep(tab);
            self.tabs.borrow_mut().clear();
            self.active.set(None);
            self.new_tab();
            return;
        }
        self.sleep(tab);
        self.tabs.borrow_mut().remove(index);
        if tab.shy && !self.tabs.borrow().iter().any(|t| t.shy) {
            self.web.drop_shy_session();
        }
        if was_active {
            self.active.set(None);
            let next = {
                let tabs = self.tabs.borrow();
                tabs[index.min(tabs.len() - 1)].clone()
            };
            self.select(&next);
        }
        self.refresh_tabs();
        self.save_soon();
    }

    pub fn close_others(self: &Rc<Self>, keep: &Rc<Tab>) {
        self.select(keep);
        let others: Vec<Rc<Tab>> =
            self.tabs.borrow().iter().filter(|t| t.id != keep.id && t.pin.borrow().is_none()).cloned().collect();
        for t in others {
            self.close(&t);
        }
    }

    /// Ctrl+Shift+T.
    pub fn reopen(self: &Rc<Self>) {
        let Some(ghost) = self.ghosts.borrow_mut().pop() else { return };
        let tab = self.make(Some(ghost.url.clone()), &ghost.title, false);
        {
            let mut tabs = self.tabs.borrow_mut();
            let at = ghost.index.min(tabs.len());
            tabs.insert(at, tab.clone());
        }
        self.select(&tab);
    }

    pub fn can_reopen(&self) -> bool {
        !self.ghosts.borrow().is_empty()
    }

    pub fn duplicate(self: &Rc<Self>, tab: &Rc<Tab>) {
        if !tab.is_blank() {
            self.open(&tab.address(), true, Some(tab));
        }
    }

    pub fn pin(self: &Rc<Self>, tab: &Rc<Tab>) {
        if tab.shy || tab.is_blank() || tab.pin.borrow().is_some() {
            return;
        }
        *tab.pin.borrow_mut() = Some(tab.monogram());
        *tab.home.borrow_mut() = tab.url.borrow().clone();
        let pins = self.tabs.borrow().iter().filter(|t| t.pin.borrow().is_some() && t.id != tab.id).count();
        self.move_tab(tab, pins);
    }

    pub fn unpin(self: &Rc<Self>, tab: &Rc<Tab>) {
        tab.pin.take();
        tab.home.take();
        let pins = self.tabs.borrow().iter().filter(|t| t.pin.borrow().is_some()).count();
        self.move_tab(tab, pins);
    }

    /// The live pin clicked again: back to where it was pinned, or, already
    /// there, its letter to change.
    pub fn go_home(self: &Rc<Self>, tab: &Rc<Tab>) {
        let home = tab.home.borrow().clone();
        match home {
            Some(h) if crate::history::identity(&h) != crate::history::identity(&tab.address()) => self.go(&h),
            _ => {
                if let Some(list) = self.ui().tabs.borrow().as_ref() {
                    list.edit_letter(tab);
                }
            }
        }
    }

    pub fn put_to_sleep(self: &Rc<Self>, tab: &Rc<Tab>) {
        if self.is_active(tab) {
            self.announce("Stays awake: it's the tab you're on");
        } else if tab.noisy.get() {
            self.announce("Stays awake: it's playing sound");
        } else {
            self.sleep(tab);
            self.changed(tab, Change::Sleep);
        }
    }

    pub fn copy_address(&self) {
        if let Some(tab) = self.active().filter(|t| !t.is_blank()) {
            self.window.clipboard().set_text(&tab.address());
            self.announce("Address copied");
        }
    }

    pub fn copy_markdown(&self, tab: &Tab) {
        let title = tab.label().replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]");
        self.window.clipboard().set_text(&format!("[{title}]({})", tab.address()));
        self.announce("Link copied");
    }

    pub fn paste_and_go(self: &Rc<Self>) {
        let weak = self.weak();
        self.window.clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |text| {
            let (Some(b), Ok(Some(text))) = (weak.upgrade(), text) else { return };
            let url = b.prefs.borrow().destination(&text);
            if let Some(url) = url {
                b.go(&url);
            }
        });
    }

    // MARK: zoom

    pub fn zoom_for(&self, url: &str) -> f64 {
        let p = self.prefs.borrow();
        crate::curtain::host_key(url).and_then(|h| p.zooms.get(&h).copied()).unwrap_or(p.page_zoom)
    }

    pub fn zoom(&self, factor: Option<f64>) {
        let Some(tab) = self.active() else { return };
        let level = match factor {
            Some(f) => (tab.zoom.get() * f).clamp(0.3, 5.0),
            None => self.prefs.borrow().page_zoom,
        };
        tab.zoom.set(level);
        if let Some(v) = tab.view.borrow().as_ref() {
            v.set_zoom_level(level);
        }
        if let Some(host) = tab.host() {
            let mut p = self.prefs.borrow_mut();
            if factor.is_none() {
                p.zooms.remove(&host);
            } else {
                p.zooms.insert(host, level);
            }
            p.save();
        }
        self.announce(&format!("{}%", (level * 100.0).round()));
    }

    // MARK: hiding things

    /// Ctrl+Shift+H: pointing mode, where a click takes a thing off the page.
    pub fn toggle_hiding(self: &Rc<Self>) {
        let on = !self.veiling.get();
        let Some(tab) = self.active().filter(|t| t.view.borrow().is_some()) else { return };
        self.veiling.set(on);
        tab.js(if on {
            "window.__wispVeil && window.__wispVeil.on()"
        } else {
            "window.__wispVeil && window.__wispVeil.off()"
        });
        self.ui().bars.hint(on);
        if let (true, Some(view)) = (on, tab.view.borrow().as_ref()) {
            view.grab_focus();
        }
    }

    pub fn picked(self: &Rc<Self>, tab: &Rc<Tab>, msg: &serde_json::Value) {
        if msg.get("off").is_some() {
            self.veiling.set(false);
            self.ui().bars.hint(false);
            return;
        }
        if let Some(trouble) = msg.get("trouble").and_then(|t| t.as_str()) {
            self.announce(&format!("Couldn't hide that — {trouble}"));
            return;
        }
        let Some(selector) = msg.get("selector").and_then(|s| s.as_str()) else { return };
        let label = msg.get("label").and_then(|s| s.as_str()).unwrap_or(selector);
        let note = msg.get("note").and_then(|s| s.as_str()).unwrap_or_default();
        let Some(host) = tab.host() else { return };
        if tab.shy {
            // A private tab writes nothing down: taken off this page only.
            let quoted = serde_json::to_string(selector).unwrap_or_default();
            tab.js(&format!(
                "document.querySelectorAll({quoted}).forEach(e=>e.style.setProperty('display','none','important'))"
            ));
            self.announce("Hidden on this page — Ctrl+Z puts it back");
            return;
        }
        self.curtain.borrow_mut().hide(&host, selector, label, note);
        self.tune(tab, &tab.address(), None);
        self.announce("Hidden — Ctrl+Z puts it back");
        self.ui().panels.refresh_hidden();
    }

    pub fn undo_hiding(self: &Rc<Self>) {
        let Some(tab) = self.active() else { return };
        let Some(host) = tab.host() else { return };
        if self.curtain.borrow_mut().undo(&host).is_some() {
            self.tune(&tab, &tab.address(), None);
            self.ui().panels.refresh_hidden();
        }
    }

    // MARK: asking

    /// A page asking to see, hear or locate you, or to send notifications:
    /// asked once per site and remembered.
    pub fn ask(self: &Rc<Self>, tab: &Rc<Tab>, request: &webkit6::PermissionRequest) -> bool {
        use webkit6::{
            GeolocationPermissionRequest, NotificationPermissionRequest, PointerLockPermissionRequest,
            UserMediaPermissionRequest,
        };
        if request.is::<PointerLockPermissionRequest>() {
            request.allow();
            return true;
        }
        let kind = if request.is::<GeolocationPermissionRequest>() {
            "location"
        } else if request.is::<NotificationPermissionRequest>() {
            if tab.shy || !self.prefs.borrow().site_notifications {
                request.deny();
                return true;
            }
            "notifications"
        } else if let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() {
            if media.is_for_video_device() { "camera" } else { "microphone" }
        } else {
            return false;
        };
        let host = tab.host().unwrap_or_else(|| "This page".into());
        let key = format!("{host} {kind}");
        if let Some(allowed) = self.prefs.borrow().permissions.get(&key).copied() {
            if allowed {
                request.allow()
            } else {
                request.deny()
            }
            return true;
        }
        let text = match kind {
            "location" => format!("{host} wants to know your location"),
            "notifications" => format!("{host} wants to send you notifications"),
            other => format!("{host} wants to use your {other}"),
        };
        let weak = self.weak();
        let request = request.clone();
        let remember = !tab.shy;
        self.ui().bars.ask(kind, &text, move |allowed| {
            if allowed {
                request.allow()
            } else {
                request.deny()
            }
            if let (Some(b), true) = (weak.upgrade(), remember) {
                b.prefs.borrow_mut().permissions.insert(key.clone(), allowed);
                b.prefs.borrow().save();
            }
        });
        true
    }

    // MARK: downloads

    pub fn watch_downloads(self: &Rc<Self>, session: &webkit6::NetworkSession) {
        let weak = self.weak();
        session.connect_download_started(move |_, download| {
            if let Some(b) = weak.upgrade() {
                b.fetch(download);
            }
        });
    }

    fn fetch(self: &Rc<Self>, download: &webkit6::Download) {
        let from =
            download.web_view().and_then(|v| v.uri()).and_then(|u| crate::address::bare_host(&u)).unwrap_or_default();
        self.fetches.borrow_mut().push(Fetch {
            download: download.clone(),
            name: String::new(),
            path: String::new(),
            from,
            failed: None,
        });
        let weak = self.weak();
        download.connect_decide_destination(move |d, suggested| {
            let dir = store::downloads_dir();
            let _ = std::fs::create_dir_all(&dir);
            let path = crate::loot::free_name(&dir, suggested);
            d.set_destination(&path.to_string_lossy());
            if let Some(b) = weak.upgrade() {
                if let Some(f) = b.fetches.borrow_mut().iter_mut().find(|f| &f.download == d) {
                    f.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    f.path = path.to_string_lossy().into_owned();
                }
                b.fetches_changed();
            }
            true
        });
        let weak = self.weak();
        download.connect_estimated_progress_notify(move |_| {
            if let Some(b) = weak.upgrade() {
                b.fetches_changed();
            }
        });
        let weak = self.weak();
        download.connect_finished(move |d| {
            let Some(b) = weak.upgrade() else { return };
            let done = {
                let mut fetches = b.fetches.borrow_mut();
                let i = fetches.iter().position(|f| &f.download == d);
                i.filter(|&i| fetches[i].failed.is_none()).map(|i| fetches.remove(i))
            };
            if let Some(f) = done {
                b.loot.borrow_mut().add(Keep {
                    name: f.name.clone(),
                    from: f.from,
                    path: f.path.clone(),
                    date: crate::history::now(),
                });
                b.ui().bars.announce(&format!("Downloaded {}", f.name), Some(f.path));
            }
            b.fetches_changed();
        });
        let weak = self.weak();
        download.connect_failed(move |d, err| {
            let Some(b) = weak.upgrade() else { return };
            let cancelled = err.matches(webkit6::DownloadError::CancelledByUser);
            {
                let mut fetches = b.fetches.borrow_mut();
                if cancelled {
                    fetches.retain(|f| &f.download != d);
                } else if let Some(f) = fetches.iter_mut().find(|f| &f.download == d) {
                    f.failed = Some(err.message().to_string());
                }
            }
            if !cancelled {
                b.announce(&format!("Download failed — {}", err.message()));
            }
            b.fetches_changed();
        });
    }

    pub fn fetches_changed(self: &Rc<Self>) {
        if let Some(list) = self.ui().tabs.borrow().as_ref() {
            list.update_fetch_door();
        }
        self.ui().panels.refresh_downloads();
    }

    // MARK: session

    pub fn save_soon(self: &Rc<Self>) {
        if self.saving.replace(true) {
            return;
        }
        let weak = self.weak();
        glib::timeout_add_local_once(Duration::from_millis(800), move || {
            if let Some(b) = weak.upgrade() {
                b.saving.set(false);
                b.save_now();
            }
        });
    }

    fn save_now(&self) {
        let tabs = self.tabs.borrow();
        let kept: Vec<&Rc<Tab>> = tabs.iter().filter(|t| !t.shy && !t.is_blank()).collect();
        let active = kept.iter().position(|t| Some(t.id) == self.active.get()).unwrap_or(0);
        let saved = Saved {
            tabs: kept
                .iter()
                .map(|t| SavedTab {
                    url: t.address(),
                    title: t.title.borrow().clone(),
                    pin: t.pin.borrow().clone(),
                    home: t.home.borrow().clone(),
                    name: t.name.borrow().clone(),
                })
                .collect(),
            active,
        };
        let _ = store::save(&store::data_dir().join("session.json"), &saved);
    }

    fn restore(self: &Rc<Self>) {
        let saved: Saved = store::load(&store::data_dir().join("session.json"));
        let fresh = self.prefs.borrow().starts_fresh;
        let mut active = None;
        for (i, s) in saved.tabs.iter().enumerate() {
            if fresh && s.pin.is_none() {
                continue;
            }
            let tab = self.make(Some(s.url.clone()), &s.title, false);
            *tab.pin.borrow_mut() = s.pin.clone();
            *tab.home.borrow_mut() = s.home.clone();
            *tab.name.borrow_mut() = s.name.clone();
            if i == saved.active {
                active = Some(tab.clone());
            }
            self.tabs.borrow_mut().push(tab);
        }
        self.refresh_tabs();
        match active.or_else(|| self.tabs.borrow().first().cloned()) {
            Some(t) if !fresh => self.select(&t),
            _ => self.new_tab(),
        }
    }

    /// A link from another program: into an empty tab on screen, or a new one.
    pub fn open_from_outside(self: &Rc<Self>, uri: &str) {
        if self.active().is_some_and(|t| t.is_blank() && !t.shy) {
            self.go(uri);
        } else {
            self.open(uri, true, None);
        }
        self.window.present();
    }

    fn start_shield(self: &Rc<Self>) {
        let weak = self.weak();
        self.web.compile_shield(move |result| {
            let Some(b) = weak.upgrade() else { return };
            match result {
                Ok(()) => b.retune(),
                Err(err) => b.announce(&format!("The ad blocker couldn't start — {err}")),
            }
        });
    }

    fn start_timers(self: &Rc<Self>) {
        let weak = self.weak();
        glib::timeout_add_seconds_local(60, move || {
            let Some(b) = weak.upgrade() else { return glib::ControlFlow::Break };
            b.history.borrow_mut().save();
            if b.prefs.borrow().sleeps_tabs {
                let idle: Vec<Rc<Tab>> = b
                    .tabs
                    .borrow()
                    .iter()
                    .filter(|t| {
                        !b.is_active(t)
                            && t.view.borrow().is_some()
                            && t.pin.borrow().is_none()
                            && !t.noisy.get()
                            && t.touched.get().elapsed() > Duration::from_secs(30 * 60)
                    })
                    .cloned()
                    .collect();
                for t in idle {
                    b.sleep(&t);
                    b.changed(&t, Change::Sleep);
                }
            }
            glib::ControlFlow::Continue
        });
    }

    pub fn set_look(&self, look: Look) {
        self.prefs.borrow_mut().look = look;
        self.prefs.borrow().save();
        apply_look(look);
    }
}

pub fn apply_look(look: Look) {
    adw::StyleManager::default().set_color_scheme(match look {
        Look::System => adw::ColorScheme::Default,
        Look::Light => adw::ColorScheme::ForceLight,
        Look::Dark => adw::ColorScheme::ForceDark,
    });
}

/// What a page that didn't load says instead, and a way to try again.
fn trouble_page() -> (gtk::Box, gtk::Label, gtk::Button) {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 10);
    page.add_css_class("ground");
    page.set_valign(gtk::Align::Fill);
    page.set_halign(gtk::Align::Fill);
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 10);
    inner.set_valign(gtk::Align::Center);
    inner.set_vexpand(true);
    let text = gtk::Label::new(None);
    text.add_css_class("trouble");
    text.set_wrap(true);
    text.set_justify(gtk::Justification::Center);
    let retry = gtk::Button::with_label("Try again");
    retry.add_css_class("plain-link");
    retry.set_halign(gtk::Align::Center);
    inner.append(&text);
    inner.append(&retry);
    page.append(&inner);
    page.set_vexpand(true);
    (page, text, retry)
}
