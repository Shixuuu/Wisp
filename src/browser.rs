//! The window: a row of tabs (down the left or across the top), one field,
//! and the page. Nothing else.

use crate::bookmarks::Bookmarks;
use crate::curtain::Curtain;
use crate::history::History;
use crate::session::{SavedTab, Session};
use crate::settings::{Settings, Theme};
use crate::tab::Tab;
use crate::web::Web;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};
use webkit6::prelude::*;

/// How many closed tabs Ctrl+Shift+T can bring back.
const REOPEN_DEPTH: usize = 25;

pub struct Browser {
    pub app: adw::Application,
    pub window: adw::ApplicationWindow,
    pub prefs: RefCell<Settings>,
    pub history: RefCell<History>,
    pub bookmarks: RefCell<Bookmarks>,
    pub curtain: RefCell<Curtain>,
    pub web: Rc<Web>,

    pub tabs: RefCell<Vec<Rc<Tab>>>,
    pub current: RefCell<Option<Rc<Tab>>>,
    closed: RefCell<Vec<SavedTab>>,
    next_id: Cell<u64>,
    pub picking: Cell<bool>,
    session_dirty: Cell<bool>,

    // Widgets.
    pub toasts: adw::ToastOverlay,
    pub root: gtk::Box,
    pub paned: gtk::Paned,
    pub sidebar: gtk::Box,
    pub topbar: gtk::Box,
    pub topbar_handle: gtk::WindowHandle,
    pub side_head: gtk::WindowHandle,
    pub page_area: gtk::Box,
    pub field: gtk::Entry,
    pub field_holder: gtk::Box,
    pub pins: gtk::FlowBox,
    pub tab_list: gtk::Box,
    pub tab_scroll: gtk::ScrolledWindow,
    pub stack: gtk::Stack,
    pub progress: gtk::ProgressBar,
    pub status: gtk::Label,
    pub find_bar: gtk::Revealer,
    pub find_entry: gtk::SearchEntry,
    pub find_count: gtk::Label,
    pub suggestions: gtk::Popover,
    pub suggestion_list: gtk::ListBox,
    pub suggestion_urls: RefCell<Vec<crate::omnibox::Pick>>,
    pub field_typed: RefCell<String>,
    pub field_quiet: Cell<bool>,
    pub new_tab_button: gtk::Button,
    pub downloads: RefCell<Vec<crate::downloads::Item>>,
    pub fullscreen: Cell<bool>,
}

impl Browser {
    pub fn new(app: &adw::Application) -> Rc<Browser> {
        let prefs = Settings::load();
        let web = Rc::new(Web::new(&prefs));
        apply_theme(prefs.theme);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Wisp")
            .default_width(1280)
            .default_height(820)
            .build();
        window.add_css_class("wisp");

        let field = gtk::Entry::builder()
            .placeholder_text("Search or enter address")
            .hexpand(true)
            .input_purpose(gtk::InputPurpose::Url)
            .build();
        field.add_css_class("wisp-field");
        let field_holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        field_holder.append(&field);

        let suggestion_list = gtk::ListBox::new();
        suggestion_list.set_selection_mode(gtk::SelectionMode::Single);
        suggestion_list.add_css_class("wisp-suggestions");
        let suggestions = gtk::Popover::builder()
            .autohide(false)
            .has_arrow(false)
            .can_focus(false)
            .position(gtk::PositionType::Bottom)
            .child(&suggestion_list)
            .build();
        suggestions.set_parent(&field);
        suggestions.set_halign(gtk::Align::Start);

        let pins = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(8)
            .row_spacing(4)
            .column_spacing(4)
            .build();
        pins.add_css_class("wisp-pins");

        let tab_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        tab_list.add_css_class("wisp-tabs");
        let tab_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&tab_list)
            .build();

        let new_tab_button = gtk::Button::from_icon_name("list-add-symbolic");
        new_tab_button.add_css_class("flat");
        new_tab_button.set_tooltip_text(Some("New tab (Ctrl+T)"));
        new_tab_button.set_action_name(Some("win.new-tab"));

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        let empty = adw::StatusPage::builder()
            .icon_name("web-browser-symbolic")
            .title("Wisp")
            .description("Type an address or a few words")
            .build();
        empty.add_css_class("wisp-empty");
        stack.add_named(&empty, Some("empty"));

        let progress = gtk::ProgressBar::new();
        progress.add_css_class("osd");
        progress.add_css_class("wisp-progress");
        progress.set_valign(gtk::Align::Start);
        progress.set_visible(false);
        progress.set_can_target(false);

        let status = gtk::Label::new(None);
        status.add_css_class("wisp-status");
        status.set_halign(gtk::Align::Start);
        status.set_valign(gtk::Align::End);
        status.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        status.set_max_width_chars(80);
        status.set_visible(false);
        status.set_can_target(false);

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&stack));
        overlay.add_overlay(&progress);
        overlay.add_overlay(&status);

        let find_entry = gtk::SearchEntry::new();
        find_entry.set_placeholder_text(Some("Find in page"));
        find_entry.set_width_chars(28);
        let find_count = gtk::Label::new(None);
        find_count.add_css_class("dim-label");
        find_count.add_css_class("numeric");
        let find_prev = gtk::Button::from_icon_name("go-up-symbolic");
        find_prev.set_action_name(Some("win.find-previous"));
        find_prev.add_css_class("flat");
        let find_next = gtk::Button::from_icon_name("go-down-symbolic");
        find_next.set_action_name(Some("win.find-next"));
        find_next.add_css_class("flat");
        let find_close = gtk::Button::from_icon_name("window-close-symbolic");
        find_close.set_action_name(Some("win.find-close"));
        find_close.add_css_class("flat");
        let find_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        find_box.add_css_class("wisp-find");
        find_box.append(&find_entry);
        find_box.append(&find_count);
        find_box.append(&find_prev);
        find_box.append(&find_next);
        find_box.append(&find_close);
        let find_bar =
            gtk::Revealer::builder().transition_type(gtk::RevealerTransitionType::SlideDown).child(&find_box).build();

        let page_area = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page_area.append(&find_bar);
        page_area.append(&overlay);

        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 6);
        sidebar.add_css_class("wisp-sidebar");
        let side_head = gtk::WindowHandle::new();
        let topbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        topbar.add_css_class("wisp-topbar");
        let topbar_handle = gtk::WindowHandle::new();
        topbar_handle.set_child(Some(&topbar));

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_shrink_start_child(false);
        paned.set_resize_start_child(false);
        paned.set_wide_handle(false);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&root));
        window.set_content(Some(&toasts));

        let browser = Rc::new(Browser {
            app: app.clone(),
            window,
            prefs: RefCell::new(prefs),
            history: RefCell::new(History::load()),
            bookmarks: RefCell::new(Bookmarks::load()),
            curtain: RefCell::new(Curtain::load()),
            web,
            tabs: RefCell::new(Vec::new()),
            current: RefCell::new(None),
            closed: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
            picking: Cell::new(false),
            session_dirty: Cell::new(false),
            toasts,
            root,
            paned,
            sidebar,
            topbar,
            topbar_handle,
            side_head,
            page_area,
            field,
            field_holder,
            pins,
            tab_list,
            tab_scroll,
            stack,
            progress,
            status,
            find_bar,
            find_entry,
            find_count,
            suggestions,
            suggestion_list,
            suggestion_urls: RefCell::new(Vec::new()),
            field_typed: RefCell::new(String::new()),
            field_quiet: Cell::new(false),
            new_tab_button,
            downloads: RefCell::new(Vec::new()),
            fullscreen: Cell::new(false),
        });

        browser.arrange();
        browser.install_actions();
        browser.wire_field();
        browser.wire_find();
        browser.wire_downloads();
        browser.start_shield();
        browser.start_timers();
        browser.restore_session();

        let weak = browser.weak();
        browser.window.connect_close_request(move |_| {
            if let Some(b) = weak.upgrade() {
                b.shutdown();
            }
            glib::Propagation::Proceed
        });
        let weak = browser.weak();
        browser.paned.connect_position_notify(move |p| {
            if let Some(b) = weak.upgrade()
                && b.prefs.borrow().tabs_on_left
                && p.position() > 120
            {
                b.prefs.borrow_mut().sidebar_width = p.position();
            }
        });

        browser
    }

    pub fn weak(self: &Rc<Self>) -> Weak<Self> {
        Rc::downgrade(self)
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn toast(&self, text: &str) {
        let toast = adw::Toast::new(text);
        toast.set_timeout(3);
        self.toasts.add_toast(toast);
    }

    // MARK: layout

    /// Put the pieces where the settings say: tabs down the left in a
    /// sidebar, or across the top in one bar. Called at start and whenever
    /// the layout or the fold changes.
    pub fn arrange(self: &Rc<Self>) {
        let (left, folded, width) = {
            let p = self.prefs.borrow();
            (p.tabs_on_left, p.sidebar_folded, p.sidebar_width)
        };
        let fullscreen = self.fullscreen.get();

        // Take everything out of wherever it was.
        for w in [
            self.field_holder.upcast_ref::<gtk::Widget>(),
            self.pins.upcast_ref(),
            self.tab_scroll.upcast_ref(),
            self.page_area.upcast_ref(),
            self.new_tab_button.upcast_ref(),
            self.side_head.upcast_ref(),
            self.sidebar.upcast_ref(),
            self.paned.upcast_ref(),
            self.topbar_handle.upcast_ref(),
        ] {
            detach(w);
        }
        for container in [&self.sidebar, &self.topbar, &self.root] {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }
        self.side_head.set_child(None::<&gtk::Widget>);

        let controls_start = gtk::WindowControls::new(gtk::PackType::Start);
        let controls_end = gtk::WindowControls::new(gtk::PackType::End);

        if left {
            self.tab_list.set_orientation(gtk::Orientation::Vertical);
            self.tab_list.set_homogeneous(false);
            self.tab_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            self.tab_scroll.set_vexpand(true);
            self.tab_scroll.set_hexpand(false);
            self.pins.set_orientation(gtk::Orientation::Horizontal);
            self.pins.set_max_children_per_line(6);
            self.field_holder.set_hexpand(true);
            self.field.set_width_chars(-1);

            let head = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            head.append(&controls_start);
            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);
            head.append(&spacer);
            head.append(&self.new_tab_button);
            head.append(&controls_end);
            self.side_head.set_child(Some(&head));

            self.sidebar.append(&self.side_head);
            self.sidebar.append(&self.field_holder);
            self.sidebar.append(&self.pins);
            self.sidebar.append(&self.tab_scroll);
            self.sidebar.set_visible(!folded && !fullscreen);

            self.paned.set_start_child(Some(&self.sidebar));
            self.paned.set_end_child(Some(&self.page_area));
            self.paned.set_position(width.clamp(160, 520));
            self.root.append(&self.paned);
            self.sidebar.add_css_class("left");
        } else {
            self.tab_list.set_orientation(gtk::Orientation::Horizontal);
            // Equal widths, shrinking together as tabs are added.
            self.tab_list.set_homogeneous(true);
            self.tab_scroll.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
            self.tab_scroll.set_vexpand(false);
            self.tab_scroll.set_hexpand(true);
            self.pins.set_orientation(gtk::Orientation::Vertical);
            self.pins.set_max_children_per_line(1);
            self.field_holder.set_hexpand(false);
            self.field.set_width_chars(34);

            self.topbar.append(&controls_start);
            self.topbar.append(&self.field_holder);
            self.topbar.append(&self.pins);
            self.topbar.append(&self.tab_scroll);
            self.topbar.append(&self.new_tab_button);
            self.topbar.append(&controls_end);
            self.topbar_handle.set_visible(!folded && !fullscreen);
            self.root.append(&self.topbar_handle);
            self.root.append(&self.page_area);
        }
        self.refresh_pins_visibility();
    }

    fn refresh_pins_visibility(&self) {
        self.pins.set_visible(self.pins.first_child().is_some());
    }

    /// Hide or show the tabs and the field, leaving only the page.
    pub fn set_chrome_visible(&self, visible: bool) {
        if self.prefs.borrow().tabs_on_left {
            self.sidebar.set_visible(visible);
        } else {
            self.topbar_handle.set_visible(visible);
        }
    }

    // MARK: tabs

    fn make_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    pub fn tab_by_id(&self, id: u64) -> Option<Rc<Tab>> {
        self.tabs.borrow().iter().find(|t| t.id == id).cloned()
    }

    pub fn current_view(&self) -> Option<webkit6::WebView> {
        self.current.borrow().as_ref().and_then(|t| t.view.borrow().clone())
    }

    /// Add a tab to the end of the list (or right after the current one when
    /// `beside` is set, as a link opened in the background is).
    pub fn add_tab(self: &Rc<Self>, url: &str, title: &str, pinned: bool, private: bool, beside: bool) -> Rc<Tab> {
        let zoom = self.prefs.borrow().default_zoom;
        let tab = Rc::new(Tab::new(self.make_id(), url, title, pinned, private, zoom));
        tab.set_icon(None);
        self.wire_row(&tab);
        {
            let mut tabs = self.tabs.borrow_mut();
            let at = if beside {
                self.current
                    .borrow()
                    .as_ref()
                    .and_then(|c| tabs.iter().position(|t| t.id == c.id))
                    .map(|i| i + 1)
                    .unwrap_or(tabs.len())
            } else {
                tabs.len()
            };
            tabs.insert(at, tab.clone());
        }
        self.place_rows();
        self.mark_session();
        tab
    }

    /// Put every row in the list in the order of `tabs`, pinned ones in the
    /// pin area.
    pub fn place_rows(&self) {
        let tabs = self.tabs.borrow();
        for tab in tabs.iter() {
            self.detach_row(&tab.row.root);
        }
        for tab in tabs.iter() {
            if tab.pinned.get() {
                self.pins.append(&tab.row.root);
            } else {
                self.tab_list.append(&tab.row.root);
            }
        }
        drop(tabs);
        self.refresh_pins_visibility();
    }

    fn detach_row(&self, row: &gtk::Box) {
        if let Some(parent) = row.parent() {
            if let Some(child) = parent.downcast_ref::<gtk::FlowBoxChild>() {
                child.set_child(None::<&gtk::Widget>);
                self.pins.remove(child);
            } else {
                detach(row.upcast_ref());
            }
        }
    }

    fn wire_row(self: &Rc<Self>, tab: &Rc<Tab>) {
        let id = tab.id;

        let click = gtk::GestureClick::new();
        click.set_button(0);
        let weak = self.weak();
        click.connect_pressed(move |gesture, _, x, y| {
            let Some(b) = weak.upgrade() else { return };
            match gesture.current_button() {
                gdk::BUTTON_MIDDLE => b.close_tab(id),
                gdk::BUTTON_SECONDARY => b.tab_menu(id, x, y),
                _ => {
                    if let Some(tab) = b.tab_by_id(id) {
                        b.select(&tab);
                    }
                }
            }
        });
        tab.row.root.add_controller(click);

        let weak = self.weak();
        tab.row.close.connect_clicked(move |_| {
            if let Some(b) = weak.upgrade() {
                b.close_tab(id);
            }
        });

        // Drag a tab onto another to move it there.
        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::MOVE);
        drag.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&id.to_string().to_value())));
        tab.row.root.add_controller(drag);
        let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        let weak = self.weak();
        drop.connect_drop(move |_, value, _, _| {
            let Some(b) = weak.upgrade() else { return false };
            let Ok(Some(from)) = value.get::<Option<String>>() else { return false };
            let Ok(from) = from.parse::<u64>() else { return false };
            b.move_tab(from, id);
            true
        });
        tab.row.root.add_controller(drop);
    }

    fn move_tab(&self, from: u64, onto: u64) {
        if from == onto {
            return;
        }
        {
            let mut tabs = self.tabs.borrow_mut();
            let Some(i) = tabs.iter().position(|t| t.id == from) else { return };
            let moving = tabs.remove(i);
            let Some(j) = tabs.iter().position(|t| t.id == onto) else {
                tabs.insert(i, moving);
                return;
            };
            // Dropping onto a pinned tab pins it; onto an ordinary one unpins.
            moving.pinned.set(tabs[j].pinned.get());
            moving.refresh_row();
            let at = if j >= i { j + 1 } else { j };
            tabs.insert(at, moving);
        }
        self.place_rows();
        self.mark_session();
    }

    fn tab_menu(self: &Rc<Self>, id: u64, x: f64, y: f64) {
        let Some(tab) = self.tab_by_id(id) else { return };
        let menu = gio::Menu::new();
        let target = id.to_variant();
        let item = |label: &str, action: &str| {
            let i = gio::MenuItem::new(Some(label), None);
            i.set_action_and_target_value(Some(action), Some(&target));
            i
        };
        menu.append_item(&item(if tab.pinned.get() { "Unpin" } else { "Pin" }, "win.tab-pin"));
        menu.append_item(&item("Duplicate", "win.tab-duplicate"));
        menu.append_item(&item("Reload", "win.tab-reload"));
        if tab.is_playing_audio() || tab.view.borrow().as_ref().is_some_and(|v| v.is_muted()) {
            menu.append_item(&item("Mute / Unmute", "win.tab-mute"));
        }
        let closing = gio::Menu::new();
        closing.append_item(&item("Close", "win.tab-close"));
        closing.append_item(&item("Close Other Tabs", "win.tab-close-others"));
        menu.append_section(None, &closing);
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(&tab.row.root);
        popover.set_has_arrow(false);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }

    /// Show a tab: build its page if it hasn't one, and bring it forward.
    pub fn select(self: &Rc<Self>, tab: &Rc<Tab>) {
        if let Some(old) = self.current.borrow().as_ref()
            && old.id != tab.id
        {
            old.row.set_current(false);
            old.last_seen.set(Instant::now());
        }
        if self.picking.get() {
            self.stop_picking();
        }
        self.close_find();
        *self.current.borrow_mut() = Some(tab.clone());
        tab.row.set_current(true);
        tab.last_seen.set(Instant::now());

        let url = tab.url.borrow().clone();
        if tab.view.borrow().is_none() && !url.is_empty() {
            let view = self.make_view(tab, None);
            view.load_uri(&url);
        }
        let has_view = tab.view.borrow().is_some();
        let page = if has_view { tab.id.to_string() } else { "empty".to_string() };
        self.stack.set_visible_child_name(&page);
        tab.refresh_row();
        self.show_address();
        self.update_progress();
        self.window.set_title(Some(&tab.name()));
        self.status.set_visible(false);
        self.scroll_to(tab);
        let view = tab.view.borrow().clone();
        match view {
            Some(view) if !self.field.has_focus() => {
                view.grab_focus();
            }
            None => {
                self.field.grab_focus();
            }
            _ => {}
        }
        self.mark_session();
    }

    fn scroll_to(&self, tab: &Rc<Tab>) {
        let row = tab.row.root.clone();
        let scroll = self.tab_scroll.clone();
        glib::idle_add_local_once(move || {
            if let Some(bounds) = row.compute_bounds(&scroll) {
                let (adj, start, size) = if scroll.vscrollbar_policy() == gtk::PolicyType::Never {
                    (scroll.hadjustment(), bounds.x() as f64, bounds.width() as f64)
                } else {
                    (scroll.vadjustment(), bounds.y() as f64, bounds.height() as f64)
                };
                let value = adj.value();
                if start < 0.0 {
                    adj.set_value(value + start);
                } else if start + size > adj.page_size() {
                    adj.set_value(value + start + size - adj.page_size());
                }
            }
        });
    }

    pub fn new_tab(self: &Rc<Self>, private: bool) {
        let tab = self.add_tab("", "", false, private, false);
        self.select(&tab);
        self.field.grab_focus();
    }

    /// Open an address in a tab: the current one if it is empty, otherwise a
    /// new one.
    pub fn open(self: &Rc<Self>, url: &str, background: bool) {
        let private = self.current.borrow().as_ref().is_some_and(|t| t.private);
        let tab = self.add_tab(url, "", false, private, true);
        if background {
            // Build it now so it starts loading, but leave it behind.
            let view = self.make_view(&tab, None);
            view.load_uri(url);
            tab.refresh_row();
        } else {
            self.select(&tab);
        }
    }

    /// Go somewhere in the current tab.
    pub fn go(self: &Rc<Self>, url: &str) {
        let current = self.current.borrow().clone();
        let tab = match current {
            Some(t) => t,
            None => self.add_tab("", "", false, false, false),
        };
        *tab.url.borrow_mut() = url.to_string();
        let existing = tab.view.borrow().clone();
        let view = match existing {
            Some(v) => v,
            None => self.make_view(&tab, None),
        };
        view.load_uri(url);
        self.select(&tab);
        view.grab_focus();
    }

    pub fn close_tab(self: &Rc<Self>, id: u64) {
        let Some(tab) = self.tab_by_id(id) else { return };
        let was_current = self.current.borrow().as_ref().is_some_and(|c| c.id == id);
        let index = self.tabs.borrow().iter().position(|t| t.id == id).unwrap_or(0);

        if !tab.private && !tab.url.borrow().is_empty() {
            let mut closed = self.closed.borrow_mut();
            closed.push(SavedTab { url: tab.url.borrow().clone(), title: tab.name(), pinned: tab.pinned.get() });
            if closed.len() > REOPEN_DEPTH {
                closed.remove(0);
            }
        }

        self.tabs.borrow_mut().retain(|t| t.id != id);
        self.discard_view(&tab);
        self.detach_row(&tab.row.root);
        self.refresh_pins_visibility();

        if tab.private && !self.tabs.borrow().iter().any(|t| t.private) {
            self.web.drop_private_session();
        }

        if was_current {
            *self.current.borrow_mut() = None;
            let next = {
                let tabs = self.tabs.borrow();
                if tabs.is_empty() { None } else { Some(tabs[index.min(tabs.len() - 1)].clone()) }
            };
            match next {
                Some(t) => self.select(&t),
                None => self.new_tab(false),
            }
        }
        self.mark_session();
    }

    pub fn reopen(self: &Rc<Self>) {
        let Some(saved) = self.closed.borrow_mut().pop() else { return };
        let tab = self.add_tab(&saved.url, &saved.title, saved.pinned, false, true);
        self.select(&tab);
    }

    pub fn duplicate(self: &Rc<Self>, id: u64) {
        let Some(tab) = self.tab_by_id(id) else { return };
        let url = tab.url.borrow().clone();
        if url.is_empty() {
            return;
        }
        let copy = self.add_tab(&url, &tab.name(), false, tab.private, true);
        self.select(&copy);
    }

    pub fn toggle_pin(self: &Rc<Self>, id: u64) {
        let Some(tab) = self.tab_by_id(id) else { return };
        tab.pinned.set(!tab.pinned.get());
        // Pinned tabs live together at the front.
        {
            let mut tabs = self.tabs.borrow_mut();
            tabs.sort_by_key(|t| !t.pinned.get());
        }
        tab.refresh_row();
        self.place_rows();
        self.mark_session();
    }

    pub fn step(self: &Rc<Self>, by: isize) {
        let next = {
            let tabs = self.tabs.borrow();
            if tabs.is_empty() {
                return;
            }
            let i = self.current.borrow().as_ref().and_then(|c| tabs.iter().position(|t| t.id == c.id)).unwrap_or(0)
                as isize;
            let n = tabs.len() as isize;
            tabs[((i + by) % n + n) as usize % n as usize].clone()
        };
        self.select(&next);
    }

    /// Ctrl+1 to Ctrl+8 go to that tab; Ctrl+9 to the last one.
    pub fn jump(self: &Rc<Self>, n: usize) {
        let target = {
            let tabs = self.tabs.borrow();
            if n >= 9 { tabs.last().cloned() } else { tabs.get(n - 1).cloned() }
        };
        if let Some(t) = target {
            self.select(&t);
        }
    }

    /// Drop a tab's page and give its memory back. Its address stays.
    pub fn discard_view(&self, tab: &Tab) {
        if let Some(view) = tab.view.borrow_mut().take() {
            view.try_close();
            self.stack.remove(&view);
        }
        tab.content.borrow_mut().take();
        tab.refresh_row();
    }

    fn start_timers(self: &Rc<Self>) {
        // Background tabs left alone long enough go to sleep; history and
        // open tabs are written down every so often.
        let weak = self.weak();
        glib::timeout_add_seconds_local(60, move || {
            let Some(b) = weak.upgrade() else { return glib::ControlFlow::Break };
            b.sleep_idle_tabs();
            b.history.borrow_mut().save();
            b.save_session_if_dirty();
            glib::ControlFlow::Continue
        });
    }

    fn sleep_idle_tabs(&self) {
        let minutes = self.prefs.borrow().sleep_minutes;
        if minutes == 0 {
            return;
        }
        let limit = Duration::from_secs(minutes as u64 * 60);
        let current = self.current.borrow().as_ref().map(|t| t.id);
        for tab in self.tabs.borrow().iter() {
            if Some(tab.id) == current || tab.view.borrow().is_none() || tab.private {
                continue;
            }
            if tab.is_playing_audio() {
                tab.last_seen.set(Instant::now());
                continue;
            }
            if tab.last_seen.get().elapsed() > limit {
                self.discard_view(tab);
            }
        }
    }

    // MARK: session

    pub fn mark_session(&self) {
        self.session_dirty.set(true);
    }

    fn save_session_if_dirty(&self) {
        if self.session_dirty.replace(false) {
            self.save_session();
        }
    }

    fn save_session(&self) {
        let tabs = self.tabs.borrow();
        let kept: Vec<&Rc<Tab>> = tabs.iter().filter(|t| !t.private && !t.url.borrow().is_empty()).collect();
        let current = self.current.borrow().as_ref().and_then(|c| kept.iter().position(|t| t.id == c.id)).unwrap_or(0);
        let session = Session {
            tabs: kept
                .iter()
                .map(|t| SavedTab { url: t.url.borrow().clone(), title: t.name(), pinned: t.pinned.get() })
                .collect(),
            current,
        };
        session.save();
    }

    fn restore_session(self: &Rc<Self>) {
        let restore = self.prefs.borrow().restore_session;
        let session = if restore { Session::load() } else { Session::default() };
        // Pinned tabs come back even when the rest don't.
        let saved: Vec<SavedTab> = if restore {
            session.tabs.clone()
        } else {
            Session::load().tabs.into_iter().filter(|t| t.pinned).collect()
        };
        for t in &saved {
            self.add_tab(&t.url, &t.title, t.pinned, false, false);
        }
        let first = self.tabs.borrow().get(session.current).cloned().or_else(|| self.tabs.borrow().first().cloned());
        match first {
            Some(t) => self.select(&t),
            None => self.new_tab(false),
        }
    }

    pub fn open_from_outside(self: &Rc<Self>, uri: &str) {
        // Reuse an empty current tab rather than leaving it behind.
        let empty = self.current.borrow().as_ref().is_some_and(|t| t.url.borrow().is_empty() && !t.private);
        if empty {
            self.go(uri);
        } else {
            let tab = self.add_tab(uri, "", false, false, false);
            self.select(&tab);
        }
        self.window.present();
    }

    fn shutdown(&self) {
        self.save_session();
        self.history.borrow_mut().save();
        self.prefs.borrow().save();
    }

    pub fn set_theme(&self, theme: Theme) {
        self.prefs.borrow_mut().theme = theme;
        apply_theme(theme);
        self.prefs.borrow().save();
    }
}

/// Take a widget out of whatever holds it, the way that container expects.
pub fn detach(w: &gtk::Widget) {
    let Some(parent) = w.parent() else { return };
    if let Some(b) = parent.downcast_ref::<gtk::Box>() {
        b.remove(w);
    } else if let Some(p) = parent.downcast_ref::<gtk::Paned>() {
        if p.start_child().as_ref() == Some(w) {
            p.set_start_child(None::<&gtk::Widget>);
        } else {
            p.set_end_child(None::<&gtk::Widget>);
        }
    } else if let Some(h) = parent.downcast_ref::<gtk::WindowHandle>() {
        h.set_child(None::<&gtk::Widget>);
    } else if let Some(s) = parent.downcast_ref::<gtk::ScrolledWindow>() {
        s.set_child(None::<&gtk::Widget>);
    } else {
        w.unparent();
    }
}

pub fn apply_theme(theme: Theme) {
    let manager = adw::StyleManager::default();
    manager.set_color_scheme(match theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
}
