//! The tabs: a column down the side, or a strip across the top.
//!
//! Every piece sits on one board and is placed by hand, so anything that
//! moves glides there: the grey that slides to the tab you picked, a row
//! making room for another, a tab carried to a new place. The same pieces
//! serve both ways of showing tabs, laid out the other way.

use crate::browser::{Browser, STRIP};
use crate::motion::{Curve, Slide, Tween};
use crate::tab::{Change, Tab};
use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::time::Duration;
use webkit6::prelude::*;

const ROW: f64 = 28.0;
const GAP: f64 = 2.0;
const SQUARE: f64 = 34.0;
const PIN_GAP: f64 = 4.0;
const TAB_WIDTH: f64 = 186.0;
const TAB_TITLED: f64 = 80.0;
const TAB_MIN: f64 = 36.0;
const PIN_WIDTH: f64 = 30.0;

/// One tab on the board.
struct Item {
    slide: Slide,
    body: gtk::Box,
    mark: gtk::Label,
    icon: gtk::Image,
    shy: gtk::Image,
    label: gtk::Label,
    spinner: gtk::Spinner,
    speaker: gtk::Button,
    edit: gtk::Text,
    x: Tween,
    y: Tween,
    width: Cell<f64>,
    height: Cell<f64>,
    hovering: Cell<bool>,
    held: Cell<bool>,
    pinned: Cell<bool>,
}

pub struct TabList {
    pub root: gtk::Widget,
    b: Weak<Browser>,
    side: bool,
    board: gtk::Fixed,
    scroller: gtk::ScrolledWindow,
    items: RefCell<HashMap<u64, Rc<Item>>>,
    pill: gtk::Box,
    fill: gtk::Box,
    pill_x: Tween,
    pill_y: Tween,
    pill_w: Tween,
    pill_h: Tween,
    new_row: gtk::Button,
    helm: [gtk::Button; 3],
    fetch: gtk::Button,
    fetch_ring: gtk::DrawingArea,
    kept: gtk::Button,
    editing: Cell<Option<u64>>,
    renaming: Cell<bool>,
    lettering: Cell<Option<u64>>,
}

impl TabList {
    pub fn new(b: &Rc<Browser>) -> TabList {
        let side = b.prefs.borrow().sidebar;
        let board = gtk::Fixed::new();
        board.set_overflow(gtk::Overflow::Visible);

        let fill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fill.add_css_class("reading");
        fill.set_halign(gtk::Align::Start);
        fill.set_hexpand(false);
        let pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        pill.add_css_class("live-ground");
        pill.set_overflow(gtk::Overflow::Hidden);
        pill.append(&fill);
        board.put(&pill, 0.0, 0.0);
        pill.set_visible(false);
        let (pill_x, pill_y) = axes(&board, &pill);
        let p = pill.clone();
        let pill_w = Tween::new(&board, 0.0, move |v| p.set_size_request(v.max(0.0) as i32, p.height_request()));
        let p = pill.clone();
        let pill_h = Tween::new(&board, 0.0, move |v| p.set_size_request(p.width_request(), v.max(0.0) as i32));

        let new_row = quiet("list-add-symbolic", "New tab");
        let weak = b.weak();
        new_row.connect_clicked(move |_| {
            if let Some(b) = weak.upgrade() {
                b.new_tab();
            }
        });
        if side {
            board.put(&new_row, 0.0, 0.0);
        }

        let helm = [
            door("go-previous-symbolic", "Back   Ctrl+["),
            door("go-next-symbolic", "Forward   Ctrl+]"),
            door("view-refresh-symbolic", "Reload   Ctrl+R"),
        ];
        for (i, d) in helm.iter().enumerate() {
            let weak = b.weak();
            d.connect_clicked(move |_| {
                let Some(b) = weak.upgrade() else { return };
                let Some(view) = b.active().and_then(|t| t.view.borrow().clone()) else { return };
                match i {
                    0 => view.go_back(),
                    1 => view.go_forward(),
                    _ if view.is_loading() => view.stop_loading(),
                    _ => view.reload(),
                }
            });
        }
        let helm_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        for d in &helm {
            helm_box.append(d);
        }

        let fetch_ring = gtk::DrawingArea::new();
        fetch_ring.set_content_width(14);
        fetch_ring.set_content_height(14);
        let fetch = door("folder-download-symbolic", "Downloads   Ctrl+Shift+J");
        fetch.set_action_name(Some("win.downloads"));
        let kept = door("user-bookmarks-symbolic", "Bookmarks");
        let weak = b.weak();
        kept.connect_clicked(move |button| {
            if let Some(b) = weak.upgrade() {
                b.ui().panels.bookmarks_dropdown(button.upcast_ref());
            }
        });
        let menu = gtk::MenuButton::new();
        menu.set_icon_name("open-menu-symbolic");
        menu.add_css_class("door");
        menu.set_menu_model(Some(&main_menu()));
        menu.set_tooltip_text(Some("Menu"));

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&board));

        let root: gtk::Widget = if side {
            let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
            column.add_css_class("side");
            column.add_css_class(if b.prefs.borrow().side_right { "right" } else { "left" });
            let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            head.set_size_request(-1, STRIP as i32);
            head.append(&gtk::WindowControls::new(gtk::PackType::Start));
            head.append(&helm_box);
            let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            space.set_hexpand(true);
            head.append(&space);
            head.append(&gtk::WindowControls::new(gtk::PackType::End));
            head.set_margin_start(10);
            head.set_margin_end(10);
            helm_box.set_valign(gtk::Align::Center);
            let handle = gtk::WindowHandle::new();
            handle.set_child(Some(&head));
            scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            scroller.set_vexpand(true);
            board.set_margin_start(10);
            board.set_margin_end(10);
            // Double-click the empty column below the rows: a new tab.
            let empty = gtk::GestureClick::new();
            let weak = b.weak();
            let bd = board.clone();
            empty.connect_pressed(move |g, n, x, y| {
                let Some(b) = weak.upgrade() else { return };
                let over = g.widget().and_then(|w| w.pick(x, y, gtk::PickFlags::DEFAULT));
                let on_board = over.is_some_and(|w| w != bd.clone().upcast::<gtk::Widget>() && w.is_ancestor(&bd));
                if n == 2 && !on_board {
                    b.new_tab();
                }
            });
            scroller.add_controller(empty);
            let foot = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            foot.add_css_class("foot");
            foot.append(&kept);
            foot.append(&fetch);
            let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            space.set_hexpand(true);
            space.append(&memory_readout());
            foot.append(&space);
            foot.append(&menu);
            column.append(&handle);
            column.append(&scroller);
            column.append(&foot);
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&column));
            overlay.add_overlay(&edge(b));
            overlay.upcast()
        } else {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, GAP as i32);
            row.add_css_class("strip");
            row.set_size_request(-1, STRIP as i32);
            row.append(&gtk::WindowControls::new(gtk::PackType::Start));
            let left = b.prefs.borrow().navigation_left;
            if left {
                row.append(&helm_box);
            }
            scroller.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
            scroller.set_propagate_natural_width(true);
            row.append(&scroller);
            let plus_button = gtk::Button::from_icon_name("list-add-symbolic");
            plus_button.add_css_class("plus");
            plus_button.set_tooltip_text(Some("New tab   Ctrl+T"));
            let weak = b.weak();
            plus_button.connect_clicked(move |_| {
                if let Some(b) = weak.upgrade() {
                    b.new_tab();
                }
            });
            let plus = crate::motion::Presence::new(&plus_button, 0.7, 0.0);
            plus.slide.set_anchor(0.0, 0.5);
            plus.root.set_valign(gtk::Align::Center);
            row.append(&plus.root);
            let space = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            space.set_hexpand(true);
            row.append(&space);
            row.append(&fetch);
            if !left {
                helm_box.set_margin_end(8);
                row.append(&helm_box);
            }
            row.append(&kept);
            row.append(&menu);
            row.append(&gtk::WindowControls::new(gtk::PackType::End));
            for w in [helm_box.upcast_ref::<gtk::Widget>(), fetch.upcast_ref(), kept.upcast_ref(), menu.upcast_ref()] {
                w.set_valign(gtk::Align::Center);
            }
            let near = gtk::EventControllerMotion::new();
            let p = plus.clone();
            near.connect_enter(move |_, _, _| p.show(true));
            let p = plus.clone();
            near.connect_leave(move |_| p.show(false));
            row.add_controller(near);
            let handle = gtk::WindowHandle::new();
            handle.set_child(Some(&row));
            // Double-click the empty strip: a new tab.
            let empty = gtk::GestureClick::new();
            let weak = b.weak();
            empty.connect_pressed(move |g, n, x, y| {
                let hit = g.widget().and_then(|w| w.pick(x, y, gtk::PickFlags::DEFAULT));
                if n == 2
                    && hit.is_some_and(|w| w.is::<gtk::Box>() && w.has_css_class("strip"))
                    && let Some(b) = weak.upgrade()
                {
                    b.new_tab();
                }
            });
            handle.add_controller(empty);
            handle.upcast()
        };
        fetch.set_visible(false);

        let list = TabList {
            root,
            b: b.weak(),
            side,
            board,
            scroller,
            items: RefCell::default(),
            pill,
            fill,
            pill_x,
            pill_y,
            pill_w,
            pill_h,
            new_row,
            helm,
            fetch,
            fetch_ring,
            kept,
            editing: Cell::new(None),
            renaming: Cell::new(false),
            lettering: Cell::new(None),
        };
        if !side {
            let weak = b.weak();
            b.window.connect_default_width_notify(move |_| {
                if let Some(b) = weak.upgrade() {
                    b.refresh_tabs();
                }
            });
            let weak = b.weak();
            b.window.connect_maximized_notify(move |_| {
                if let Some(b) = weak.upgrade() {
                    glib::idle_add_local_once(move || b.refresh_tabs());
                }
            });
        }
        list
    }

    fn browser(&self) -> Option<Rc<Browser>> {
        self.b.upgrade()
    }

    // MARK: the board

    /// Bring the board in line with the tabs: new ones in, closed ones out,
    /// every piece gliding to its place.
    pub fn sync(&self) {
        let Some(b) = self.browser() else { return };
        let tabs: Vec<Rc<Tab>> = b.tabs.borrow().clone();
        let alive: Vec<u64> = tabs.iter().map(|t| t.id).collect();
        let dead: Vec<u64> = self.items.borrow().keys().filter(|id| !alive.contains(id)).copied().collect();
        for id in dead {
            if let Some(item) = self.items.borrow_mut().remove(&id) {
                let (board, slide) = (self.board.clone(), item.slide.clone());
                crate::motion::animate(&item.slide, 1.0, 0.0, Curve::Quick, move |t| {
                    slide.set_opacity(t);
                    if t <= 0.001 {
                        // After the frame, not inside it (see motion::Place).
                        let (board, slide) = (board.clone(), slide.clone());
                        glib::idle_add_local_once(move || {
                            if slide.parent().is_some() {
                                board.remove(&slide);
                            }
                        });
                    }
                });
            }
        }
        for tab in &tabs {
            let fresh = !self.items.borrow().contains_key(&tab.id);
            if fresh {
                let item = self.make_item(&b, tab);
                self.items.borrow_mut().insert(tab.id, item);
            }
            self.dress(&b, tab);
        }
        self.relayout();
        self.update_helm();
        self.update_fetch_door();
    }

    /// Place every piece where it belongs now.
    pub fn relayout(&self) {
        let Some(b) = self.browser() else { return };
        let tabs: Vec<Rc<Tab>> = b.tabs.borrow().clone();
        let items = self.items.borrow();
        let pins: Vec<&Rc<Tab>> = tabs.iter().filter(|t| t.pin.borrow().is_some()).collect();
        let loose: Vec<&Rc<Tab>> = tabs.iter().filter(|t| t.pin.borrow().is_none()).collect();
        let place = |tab: &Rc<Tab>, x: f64, y: f64, w: f64, h: f64| {
            let Some(item) = items.get(&tab.id) else { return };
            item.width.set(w);
            item.height.set(h);
            item.slide.set_size_request(w as i32, h as i32);
            if item.held.get() {
                return;
            }
            let new = item.slide.parent().is_none();
            if new {
                self.board.put(&item.slide, x, y);
                item.x.set(x);
                item.y.set(y);
                appear(&item.slide);
            } else {
                item.x.to(x, Curve::Settle);
                item.y.to(y, Curve::Settle);
            }
        };
        if self.side {
            let room = b.prefs.borrow().side_width - 20.0;
            let cells = pin_cells(pins.len(), room);
            for (tab, cell) in pins.iter().zip(&cells) {
                place(tab, cell.0, cell.1, cell.2, cell.3);
            }
            let mut y = cells.iter().map(|c| c.1 + c.3).fold(0.0, f64::max);
            if !pins.is_empty() {
                y += 10.0;
            }
            for tab in &loose {
                place(tab, 0.0, y, room, ROW);
                y += ROW + GAP;
            }
            self.new_row.set_size_request(room as i32, ROW as i32);
            self.board.move_(&self.new_row, 0.0, y + GAP);
        } else {
            let room = self.strip_room(&b);
            let spent = pins.len() as f64 * PIN_WIDTH + (tabs.len().saturating_sub(1)) as f64 * GAP;
            let each = if loose.is_empty() {
                TAB_WIDTH
            } else {
                ((room - spent) / loose.len() as f64).clamp(TAB_MIN, TAB_WIDTH)
            };
            let editing = self.editing.get();
            let mut x = 0.0;
            let y = (STRIP - ROW) / 2.0;
            for tab in &tabs {
                let w = if tab.pin.borrow().is_some() {
                    PIN_WIDTH
                } else if editing == Some(tab.id) {
                    340.0_f64.min(room)
                } else {
                    each
                };
                place(tab, x, y, w, ROW);
                if let Some(item) = items.get(&tab.id) {
                    compact(item, w < TAB_TITLED && tab.pin.borrow().is_none() && editing != Some(tab.id));
                }
                x += w + GAP;
            }
            self.board.set_size_request(x as i32, STRIP as i32);
            self.scroller.set_max_content_width(room as i32);
            self.scroller.set_size_request((x - GAP).min(room).max(0.0) as i32, -1);
        }
        drop(items);
        self.place_pill(&b);
    }

    /// The width the strip can give its tabs.
    fn strip_room(&self, b: &Browser) -> f64 {
        let w = b.window.width().max(b.window.default_width()) as f64;
        (w - 470.0).max(TAB_MIN)
    }

    /// The grey under the tab you are on glides to it.
    fn place_pill(&self, b: &Browser) {
        let active = b.active();
        let item = active.as_ref().and_then(|t| self.items.borrow().get(&t.id).cloned());
        let (Some(tab), Some(item)) = (active, item) else {
            self.pill.set_visible(false);
            return;
        };
        let (x, y) = (item.x.target(), item.y.target());
        let (w, h) = (item.width.get(), item.height.get());
        let first = !self.pill.is_visible();
        self.pill.set_visible(true);
        let pinned = tab.pin.borrow().is_some();
        if pinned {
            self.pill.add_css_class("pin");
        } else {
            self.pill.remove_css_class("pin");
        }
        for (tween, to) in [(&self.pill_x, x), (&self.pill_y, y), (&self.pill_w, w), (&self.pill_h, h)] {
            if first { tween.set(to) } else { tween.to(to, Curve::Glide) }
        }
        let meter = if b.prefs.borrow().shows_reading && !pinned { tab.meter.get() } else { 0.0 };
        self.fill.set_size_request((meter * w) as i32, -1);
    }

    fn make_item(&self, b: &Rc<Browser>, tab: &Rc<Tab>) -> Rc<Item> {
        let body = gtk::Box::new(gtk::Orientation::Horizontal, if self.side { 8 } else { 6 });
        body.add_css_class("tab");
        let mark = gtk::Label::new(None);
        mark.add_css_class("mark");
        let icon = gtk::Image::new();
        icon.set_pixel_size(15);
        icon.add_css_class("favicon");
        icon.update_property(&[gtk::accessible::Property::Label("Site icon")]);
        let shy = gtk::Image::from_icon_name("view-conceal-symbolic");
        shy.set_pixel_size(10);
        shy.add_css_class("shy");
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.add_css_class("title");
        let edit = crate::text_box("Tab name");
        edit.set_hexpand(true);
        edit.set_visible(false);
        edit.add_css_class("tab-edit");
        let spinner = gtk::Spinner::new();
        spinner.set_size_request(13, 13);
        spinner.set_visible(false);
        let speaker = gtk::Button::from_icon_name("audio-volume-high-symbolic");
        speaker.add_css_class("speaker");
        speaker.set_visible(false);
        for w in [
            mark.upcast_ref::<gtk::Widget>(),
            icon.upcast_ref(),
            shy.upcast_ref(),
            label.upcast_ref(),
            edit.upcast_ref(),
            spinner.upcast_ref(),
            speaker.upcast_ref(),
        ] {
            body.append(w);
        }
        let slide = Slide::new(&body);
        slide.set_anchor(0.0, 0.5);
        let (x, y) = axes(&self.board, &slide);
        let item = Rc::new(Item {
            slide,
            body,
            mark,
            icon,
            shy,
            label,
            spinner,
            speaker,
            edit,
            x,
            y,
            width: Cell::new(0.0),
            height: Cell::new(ROW),
            hovering: Cell::new(false),
            held: Cell::new(false),
            pinned: Cell::new(false),
        });
        self.wire(b, tab, &item);
        item
    }

    fn wire(&self, b: &Rc<Browser>, tab: &Rc<Tab>, item: &Rc<Item>) {
        let id = tab.id;
        let side = self.side;

        let hover = gtk::EventControllerMotion::new();
        let it = Rc::downgrade(item);
        let set_hover = move |on: bool| {
            let Some(item) = it.upgrade() else { return };
            item.hovering.set(on);
            if on { item.body.add_css_class("hover") } else { item.body.remove_css_class("hover") }
        };
        let s = set_hover.clone();
        hover.connect_enter(move |_, _, _| s(true));
        hover.connect_leave(move |_| set_hover(false));
        item.slide.add_controller(hover);

        // Menu and Shift+F10 open the same menu as a right-click, so a row
        // can be pinned from the keyboard.
        item.body.set_focusable(true);
        let keys = gtk::EventControllerKey::new();
        let weak = b.weak();
        let anchor = item.slide.clone();
        keys.connect_key_pressed(move |_, key, _, mods| {
            let menu_key =
                key == gdk::Key::Menu || (key == gdk::Key::F10 && mods.contains(gdk::ModifierType::SHIFT_MASK));
            if !menu_key {
                return glib::Propagation::Proceed;
            }
            let Some(b) = weak.upgrade() else { return glib::Propagation::Proceed };
            let Some(tab) = b.tab(id) else { return glib::Propagation::Proceed };
            menu(&b, &tab, &anchor, 8.0, 8.0);
            glib::Propagation::Stop
        });
        item.body.add_controller(keys);

        let weak = b.weak();
        item.speaker.connect_clicked(move |_| {
            let Some(t) = weak.upgrade().and_then(|b| b.tab(id)) else { return };
            if let Some(v) = t.view.borrow().as_ref() {
                v.set_is_muted(!v.is_muted());
                t.muted.set(v.is_muted());
            }
            if let Some(b) = weak.upgrade() {
                b.changed(&t, Change::Sound);
            }
        });

        let click = gtk::GestureClick::new();
        click.set_button(0);
        let weak = b.weak();
        let it = Rc::downgrade(item);
        click.connect_released(move |g, n, x, y| {
            let (Some(b), Some(item)) = (weak.upgrade(), it.upgrade()) else { return };
            let Some(tab) = b.tab(id) else { return };
            if item.held.get() || WidgetExt::is_visible(&item.edit) {
                return;
            }
            match g.current_button() {
                gdk::BUTTON_MIDDLE => b.close(&tab),
                gdk::BUTTON_SECONDARY => menu(&b, &tab, &item.slide, x, y),
                _ => {
                    let live = b.is_active(&tab);
                    let pinned = tab.pin.borrow().is_some();
                    if live && pinned {
                        if n == 2 {
                            b.go_home(&tab);
                        }
                    } else if live {
                        if let Some(list) = b.ui().tabs.borrow().as_ref() {
                            list.begin_edit(&tab, false);
                        }
                    } else {
                        b.select(&tab);
                    }
                }
            }
        });
        item.slide.add_controller(click);

        // Carried to a new place: the tab stays under the hand and the
        // others make room, one place at a time.
        let drag = gtk::GestureDrag::new();
        let weak = b.weak();
        let it = Rc::downgrade(item);
        let start = Rc::new(Cell::new((0.0, 0.0)));
        let s = start.clone();
        drag.connect_drag_begin(move |_, _, _| {
            if let Some(item) = it.upgrade() {
                s.set((item.x.value(), item.y.value()));
            }
        });
        let it = Rc::downgrade(item);
        let s = start.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let (Some(b), Some(item)) = (weak.upgrade(), it.upgrade()) else { return };
            let Some(tab) = b.tab(id) else { return };
            if WidgetExt::is_visible(&item.edit) {
                return;
            }
            let travel = if side { dy } else { dx };
            if !item.held.get() && travel.abs() < 5.0 {
                return;
            }
            item.held.set(true);
            item.slide.add_css_class("held");
            let (x0, y0) = s.get();
            let (x, y) = if side && item.pinned.get() {
                (x0 + dx, y0 + dy)
            } else if side {
                (x0, y0 + dy)
            } else {
                (x0 + dx, y0)
            };
            item.x.set(x);
            item.y.set(y);
            let target = b.ui().tabs.borrow().as_ref().and_then(|l| l.target_for(&b, &tab, x, y));
            if let Some(to) = target {
                b.move_tab(&tab, to);
            }
        });
        let it = Rc::downgrade(item);
        let weak = b.weak();
        drag.connect_drag_end(move |_, _, _| {
            let Some(item) = it.upgrade() else { return };
            if !item.held.get() {
                return;
            }
            item.slide.remove_css_class("held");
            item.held.set(false);
            if let Some(b) = weak.upgrade()
                && let Some(list) = b.ui().tabs.borrow().as_ref()
            {
                list.relayout();
            }
            // The press that ended the carry is not also a click.
            item.held.set(true);
            let item = item.clone();
            glib::idle_add_local_once(move || item.held.set(false));
        });
        item.slide.add_controller(drag);

        // Typing a new address into the tab itself.
        let keys = gtk::EventControllerKey::new();
        let weak = b.weak();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(b) = weak.upgrade()
                    && let Some(list) = b.ui().tabs.borrow().as_ref()
                {
                    list.end_edit(false);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        item.edit.add_controller(keys);
        let weak = b.weak();
        item.edit.connect_activate(move |_| {
            if let Some(b) = weak.upgrade()
                && let Some(list) = b.ui().tabs.borrow().as_ref()
            {
                list.end_edit(true);
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let weak = b.weak();
        focus.connect_leave(move |_| {
            let Some(b) = weak.upgrade() else { return };
            glib::idle_add_local_once(move || {
                if let Some(list) = b.ui().tabs.borrow().as_ref() {
                    list.end_edit(true);
                }
            });
        });
        item.edit.add_controller(focus);
    }

    /// The place a carried tab is over, among its own kind (pins or not).
    fn target_for(&self, b: &Browser, tab: &Tab, x: f64, y: f64) -> Option<usize> {
        let tabs = b.tabs.borrow();
        let pinned = tab.pin.borrow().is_some();
        let here = tabs.iter().position(|t| t.id == tab.id)?;
        let items = self.items.borrow();
        let item = items.get(&tab.id)?;
        let (cx, cy) = (x + item.width.get() / 2.0, y + item.height.get() / 2.0);
        let best = tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| (t.pin.borrow().is_some()) == pinned)
            .filter_map(|(i, t)| {
                let it = items.get(&t.id)?;
                let (tx, ty) = if t.id == tab.id { (x, y) } else { (it.x.target(), it.y.target()) };
                let (mx, my) = (tx + it.width.get() / 2.0, ty + it.height.get() / 2.0);
                Some((i, (mx - cx).hypot(my - cy)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)?;
        (best != here).then_some(best)
    }

    /// Dress one tab's piece in its current state.
    fn dress(&self, b: &Browser, tab: &Rc<Tab>) {
        let Some(item) = self.items.borrow().get(&tab.id).cloned() else { return };
        let live = b.is_active(tab);
        let pinned = tab.pin.borrow().is_some();
        item.pinned.set(pinned);
        for (class, on) in [("live", live), ("pinned", pinned), ("asleep", tab.asleep()), ("side", self.side)] {
            if on { item.body.add_css_class(class) } else { item.body.remove_css_class(class) }
        }
        let loading = tab.loading.get() && !tab.asleep();
        let texture = tab.icon.borrow().clone();
        // A pin's letter is the thing the user chose. The favicon is for ordinary rows.
        let show_icon = texture.is_some() && !pinned;
        item.icon.set_paintable(texture.as_ref());
        item.icon.set_visible(show_icon);
        let mark_text = if pinned { tab.pin.borrow().clone().unwrap_or_default() } else { tab.monogram() };
        item.mark.set_label(&mark_text);
        item.mark.set_visible(!show_icon && !loading && (pinned || !tab.is_blank()));
        if pinned {
            item.mark.add_css_class("letter")
        } else {
            item.mark.remove_css_class("letter")
        }
        item.shy.set_visible(tab.shy && !pinned);
        item.label.set_label(&tab.label());
        item.label.set_visible(!pinned && !WidgetExt::is_visible(&item.edit));
        item.slide.set_tooltip_text(Some(&tab.label()));
        item.spinner.set_visible(loading);
        item.spinner.set_spinning(loading);
        let speaker = !loading && !pinned && (tab.noisy.get() || tab.muted.get());
        item.speaker.set_visible(speaker);
        item.speaker.set_icon_name(if tab.muted.get() {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        });
        if pinned {
            item.body.set_halign(gtk::Align::Center);
            item.label.set_hexpand(false);
        } else {
            item.body.set_halign(gtk::Align::Fill);
            item.label.set_hexpand(true);
        }
    }

    pub fn update(&self, tab: &Rc<Tab>, what: Change) {
        let Some(b) = self.browser() else { return };
        self.dress(&b, tab);
        match what {
            Change::Meter => self.place_pill(&b),
            Change::Address | Change::Loading => self.update_helm(),
            _ => {}
        }
    }

    pub fn update_helm(&self) {
        let Some(b) = self.browser() else { return };
        let tab = b.active();
        let live = tab.as_ref().is_some_and(|t| !t.is_blank());
        let view = tab.as_ref().and_then(|t| t.view.borrow().clone());
        let back = view.as_ref().is_some_and(|v| v.can_go_back()) && live;
        let forward = view.as_ref().is_some_and(|v| v.can_go_forward()) && live;
        let loading = view.as_ref().is_some_and(|v| v.is_loading());
        for (d, on) in self.helm.iter().zip([back, forward, live]) {
            d.set_sensitive(on);
            d.set_opacity(if on { 1.0 } else { 0.3 });
        }
        self.helm[2].set_icon_name(if loading { "process-stop-symbolic" } else { "view-refresh-symbolic" });
        self.helm[2].set_tooltip_text(Some(if loading { "Stop" } else { "Reload   Ctrl+R" }));
        let kept = tab.is_some_and(|t| !t.is_blank() && b.bookmarks.borrow().find(&t.address()).is_some());
        self.kept.set_icon_name(if kept { "starred-symbolic" } else { "user-bookmarks-symbolic" });
    }

    /// The downloads door: there while a file comes in, and a moment after.
    pub fn update_fetch_door(&self) {
        let Some(b) = self.browser() else { return };
        let fetches = b.fetches.borrow();
        let running: Vec<f64> =
            fetches.iter().filter(|f| f.failed.is_none()).map(|f| f.download.estimated_progress()).collect();
        let show = !fetches.is_empty() || b.prefs.borrow().always_shows_downloads;
        drop(fetches);
        if !running.is_empty() {
            let fraction = running.iter().sum::<f64>() / running.len() as f64;
            self.fetch_ring.set_draw_func(move |_, cr, w, h| {
                let (cx, cy, r) = (w as f64 / 2.0, h as f64 / 2.0, w.min(h) as f64 / 2.0 - 1.5);
                cr.set_line_width(1.5);
                cr.set_source_rgba(0.55, 0.55, 0.55, 0.3);
                cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
                let _ = cr.stroke();
                cr.set_source_rgba(0.35, 0.35, 0.35, 0.9);
                let start = -std::f64::consts::FRAC_PI_2;
                cr.arc(cx, cy, r, start, start + std::f64::consts::TAU * fraction.max(0.02));
                let _ = cr.stroke();
            });
            self.fetch.set_child(Some(&self.fetch_ring));
            self.fetch_ring.queue_draw();
        } else {
            self.fetch.set_icon_name("folder-download-symbolic");
        }
        if show {
            self.fetch.set_visible(true);
        } else if self.fetch.is_visible() {
            let f = self.fetch.clone();
            let weak = self.b.clone();
            glib::timeout_add_seconds_local_once(4, move || {
                let keep = weak
                    .upgrade()
                    .is_some_and(|b| !b.fetches.borrow().is_empty() || b.prefs.borrow().always_shows_downloads);
                f.set_visible(keep);
            });
        }
    }

    pub fn bookmark_door(&self) -> gtk::Widget {
        self.kept.clone().upcast()
    }

    // MARK: typing into a tab

    /// The live tab clicked: its address, ready to be typed over. Or, from
    /// its menu, its name.
    pub fn begin_edit(&self, tab: &Rc<Tab>, rename: bool) {
        if tab.is_blank() {
            if let Some(b) = self.browser() {
                b.ui().field.edit();
            }
            return;
        }
        self.end_edit(false);
        let Some(item) = self.items.borrow().get(&tab.id).cloned() else { return };
        self.editing.set(Some(tab.id));
        self.renaming.set(rename);
        let text = if rename { tab.label() } else { crate::address::editable(&tab.address()) };
        item.edit.set_text(&text);
        item.label.set_visible(false);
        item.edit.set_visible(true);
        item.edit.grab_focus();
        item.edit.select_region(0, -1);
        self.relayout();
    }

    pub fn end_edit(&self, commit: bool) {
        let Some(id) = self.editing.take() else { return };
        let Some(b) = self.browser() else { return };
        let Some(item) = self.items.borrow().get(&id).cloned() else { return };
        let draft = item.edit.text().trim().to_string();
        item.edit.set_visible(false);
        item.label.set_visible(true);
        if let (true, Some(tab)) = (commit, b.tab(id)) {
            if self.renaming.get() {
                *tab.name.borrow_mut() = (!draft.is_empty()).then_some(draft);
                b.save_soon();
            } else if !draft.is_empty() && draft != crate::address::editable(&tab.address()) {
                let url = b.prefs.borrow().destination(&draft);
                if let Some(url) = url {
                    b.select(&tab);
                    b.go(&url);
                }
            }
            self.dress(&b, &tab);
        }
        self.relayout();
        if let Some(v) = b.active().and_then(|t| t.view.borrow().clone()) {
            v.grab_focus();
        }
    }

    /// A pin's letter, to be typed over.
    pub fn edit_letter(&self, tab: &Rc<Tab>) {
        let Some(item) = self.items.borrow().get(&tab.id).cloned() else { return };
        self.lettering.set(Some(tab.id));
        item.edit.set_max_length(1);
        item.edit.set_text(&tab.pin.borrow().clone().unwrap_or_default());
        item.mark.set_visible(false);
        item.icon.set_visible(false);
        item.edit.set_visible(true);
        item.edit.set_width_chars(1);
        item.edit.grab_focus();
        item.edit.select_region(0, -1);
        let weak = self.b.clone();
        let id = tab.id;
        let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
        let h = handler.clone();
        let edit = item.edit.clone();
        *handler.borrow_mut() = Some(item.edit.connect_changed(move |e| {
            let letter: String = e.text().chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            if letter.is_empty() {
                return;
            }
            let Some(b) = weak.upgrade() else { return };
            if let Some(tab) = b.tab(id) {
                *tab.pin.borrow_mut() = Some(letter);
                if let Some(list) = b.ui().tabs.borrow().as_ref() {
                    list.lettering.set(None);
                    if let Some(item) = list.items.borrow().get(&id) {
                        item.edit.set_visible(false);
                        item.edit.set_max_length(0);
                    }
                    list.dress(&b, &tab);
                }
                b.save_soon();
            }
            if let Some(id) = h.take() {
                edit.disconnect(id);
            }
        }));
    }
}

/// Two tweens that place a child on the board, one per axis. The position is
/// kept here rather than read back from the board, which reports it late.
pub fn axes(board: &gtk::Fixed, child: &impl IsA<gtk::Widget>) -> (Tween, Tween) {
    let at = Rc::new(Cell::new((0.0, 0.0)));
    let (f, c, a) = (board.clone(), child.clone().upcast::<gtk::Widget>(), at.clone());
    let x = Tween::new(board, 0.0, move |v| {
        a.set((v, a.get().1));
        if c.parent().is_some() {
            f.move_(&c, v, a.get().1);
        }
    });
    let (f, c, a) = (board.clone(), child.clone().upcast::<gtk::Widget>(), at);
    let y = Tween::new(board, 0.0, move |v| {
        a.set((a.get().0, v));
        if c.parent().is_some() {
            f.move_(&c, a.get().0, v);
        }
    });
    (x, y)
}

/// Where each pinned square goes: at most four to a row, as even as they
/// go, the fuller rows first; a single row keeps three places.
fn pin_cells(count: usize, room: f64) -> Vec<(f64, f64, f64, f64)> {
    if count == 0 {
        return vec![];
    }
    let fits = (((room + PIN_GAP) / (SQUARE + PIN_GAP)) as usize).max(1);
    let most = fits.min(4);
    let rows = count.div_ceil(most);
    let (base, extra) = (count / rows, count % rows);
    let counts: Vec<usize> = (0..rows).map(|r| if r < extra { base + 1 } else { base }).collect();
    let slots: Vec<usize> = counts.iter().map(|&n| if rows == 1 { n.max(3.min(fits)) } else { n }).collect();
    let widths: Vec<f64> = slots.iter().map(|&n| ((room - (n as f64 - 1.0) * PIN_GAP) / n as f64).max(20.0)).collect();
    let height = widths.iter().copied().fold(SQUARE, f64::min);
    let mut cells = vec![];
    for (r, &n) in counts.iter().enumerate() {
        for c in 0..n {
            cells.push((c as f64 * (widths[r] + PIN_GAP), r as f64 * (height + PIN_GAP), widths[r], height));
        }
    }
    cells
}

/// A new piece arrives growing from its leading edge.
fn appear(slide: &Slide) {
    let s = slide.clone();
    crate::motion::animate(slide, 0.0, 1.0, Curve::Settle, move |t| {
        s.set_opacity(t.clamp(0.0, 1.0));
        s.set_scale(0.94 + 0.06 * t);
    });
}

/// A tab too narrow for its title shows its mark alone.
fn compact(item: &Item, on: bool) {
    if on {
        item.body.add_css_class("compact");
        item.label.set_visible(false);
        item.mark.set_visible(!item.icon.is_visible());
        item.body.set_halign(gtk::Align::Center);
    } else if item.body.has_css_class("compact") {
        item.body.remove_css_class("compact");
        item.label.set_visible(!item.pinned.get());
        item.body.set_halign(gtk::Align::Fill);
    }
}

/// A quiet figure for the footer. Pages live in child processes, so the
/// window's own number would leave them out.
fn memory_readout() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("mem");
    label.set_halign(gtk::Align::End);
    label.set_valign(gtk::Align::Center);
    label.set_margin_end(8);
    label.set_tooltip_text(Some("Memory in use"));
    let shown = label.clone();
    let paint = move || {
        let kib = resident_kib();
        if kib == 0 {
            shown.set_visible(false);
            return;
        }
        shown.set_visible(true);
        let text = memory_text(kib);
        if shown.text().as_str() != text {
            shown.set_text(&text);
        }
    };
    paint();
    let watch = label.clone();
    glib::timeout_add_local(Duration::from_secs(2), move || {
        if watch.parent().is_none() {
            return glib::ControlFlow::Break;
        }
        paint();
        glib::ControlFlow::Continue
    });
    label
}

/// Whole mebibytes until a gibibyte, then one decimal. Small enough to stay
/// a single glance in the footer.
fn memory_text(kib: u64) -> String {
    if kib >= 1024 * 1024 {
        format!("{:.1} GB", kib as f64 / (1024.0 * 1024.0))
    } else if kib >= 1024 {
        format!("{} MB", kib / 1024)
    } else {
        format!("{kib} KB")
    }
}

fn resident_kib() -> u64 {
    fn rss(pid: u32) -> u64 {
        let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/status")) else { return 0 };
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("VmRSS:") else { continue };
            return rest.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        }
        0
    }
    fn children(pid: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else { return out };
        for task in tasks.flatten() {
            let Ok(text) = std::fs::read_to_string(task.path().join("children")) else { continue };
            for word in text.split_whitespace() {
                if let Ok(kid) = word.parse() {
                    out.push(kid);
                }
            }
        }
        out
    }
    fn walk(pid: u32, depth: u32, seen: &mut HashSet<u32>) -> u64 {
        if depth > 6 || !seen.insert(pid) {
            return 0;
        }
        let mut total = rss(pid);
        for kid in children(pid) {
            total += walk(kid, depth + 1, seen);
        }
        total
    }
    walk(std::process::id(), 0, &mut HashSet::new())
}

/// A small square holding one symbol.
pub fn door(icon: &str, tip: &str) -> gtk::Button {
    let d = gtk::Button::from_icon_name(icon);
    d.add_css_class("door");
    d.set_tooltip_text(Some(tip));
    d
}

/// A row that is an action rather than a page. Quiet until pointed at.
fn quiet(icon: &str, title: &str) -> gtk::Button {
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(10);
    image.set_size_request(15, -1);
    inner.append(&image);
    inner.append(&gtk::Label::new(Some(title)));
    let b = gtk::Button::new();
    b.set_child(Some(&inner));
    b.add_css_class("quiet");
    b
}

/// The column's edge: pull it to resize, double-click it to put it back.
fn edge(b: &Rc<Browser>) -> gtk::Widget {
    let right = b.prefs.borrow().side_right;
    let line = gtk::Box::new(gtk::Orientation::Vertical, 0);
    line.add_css_class("edge-line");
    line.set_halign(gtk::Align::Center);
    let edge = gtk::Box::new(gtk::Orientation::Vertical, 0);
    edge.add_css_class("edge");
    edge.append(&line);
    edge.set_size_request(9, -1);
    edge.set_halign(if right { gtk::Align::Start } else { gtk::Align::End });
    edge.set_margin_end(if right { 0 } else { -4 });
    edge.set_cursor_from_name(Some("col-resize"));
    let drag = gtk::GestureDrag::new();
    let grabbed = Rc::new(Cell::new(0.0));
    let weak = b.weak();
    let g = grabbed.clone();
    let l = line.clone();
    drag.connect_drag_begin(move |_, _, _| {
        if let Some(b) = weak.upgrade() {
            g.set(b.prefs.borrow().side_width);
            l.add_css_class("held");
        }
    });
    let weak = b.weak();
    drag.connect_drag_update(move |_, dx, _| {
        if let Some(b) = weak.upgrade() {
            let delta = if right { -dx } else { dx };
            b.set_side_width(grabbed.get() + delta);
        }
    });
    let weak = b.weak();
    let l = line.clone();
    drag.connect_drag_end(move |_, _, _| {
        l.remove_css_class("held");
        if let Some(b) = weak.upgrade() {
            b.prefs.borrow().save();
        }
    });
    edge.add_controller(drag);
    let twice = gtk::GestureClick::new();
    let weak = b.weak();
    twice.connect_pressed(move |_, n, _, _| {
        if n != 2 {
            return;
        }
        let Some(b) = weak.upgrade() else { return };
        let from = b.prefs.borrow().side_width;
        let weak = b.weak();
        crate::motion::animate(&b.window, from, crate::settings::SIDE, Curve::Settle, move |w| {
            if let Some(b) = weak.upgrade() {
                b.set_side_width(w);
            }
        });
    });
    edge.add_controller(twice);
    edge.upcast()
}

/// A tab's own menu, on a right-click.
fn menu(b: &Rc<Browser>, tab: &Rc<Tab>, anchor: &Slide, x: f64, y: f64) {
    let target = tab.id.to_variant();
    let item = |label: &str, action: &str| {
        let i = gio::MenuItem::new(Some(label), None);
        i.set_action_and_target_value(Some(action), Some(&target));
        i
    };
    let menu = gio::Menu::new();
    let first = gio::Menu::new();
    if tab.pin.borrow().is_none() {
        if !tab.is_blank() && !tab.shy {
            first.append_item(&item("Pin", "win.tab-pin"));
        }
    } else {
        first.append_item(&item("Change Letter", "win.tab-letter"));
        first.append_item(&item("Unpin", "win.tab-unpin"));
    }
    menu.append_section(None, &first);
    let middle = gio::Menu::new();
    middle.append_item(&item("Rename", "win.tab-rename"));
    if !tab.is_blank() {
        middle.append_item(&item("Duplicate", "win.tab-duplicate"));
        middle.append_item(&item("Copy Address", "win.tab-copy"));
        middle.append_item(&item("Copy as Markdown Link", "win.tab-markdown"));
    }
    middle.append_item(&item(if tab.muted.get() { "Unmute Tab" } else { "Mute Tab" }, "win.tab-mute"));
    if !tab.asleep() && !tab.is_blank() {
        middle.append_item(&item("Put to Sleep", "win.tab-sleep"));
    }
    menu.append_section(None, &middle);
    let last = gio::Menu::new();
    last.append_item(&item("Close Tab", "win.tab-close"));
    if b.tabs.borrow().len() > 1 {
        last.append_item(&item("Close Other Tabs", "win.tab-close-others"));
    }
    if b.can_reopen() {
        let reopen = gio::MenuItem::new(Some("Reopen Closed Tab"), Some("win.reopen"));
        last.append_item(&reopen);
    }
    menu.append_section(None, &last);
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(anchor);
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}

/// What the macOS menu bar gave Search, in the one place Linux has for it.
fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let section = |items: &[(&str, &str)]| {
        let s = gio::Menu::new();
        for (label, action) in items {
            s.append(Some(label), Some(action));
        }
        s
    };
    menu.append_section(
        None,
        &section(&[
            ("New Tab", "win.new-tab"),
            ("New Private Tab", "win.new-private-tab"),
            ("Reopen Closed Tab", "win.reopen"),
        ]),
    );
    menu.append_section(
        None,
        &section(&[
            ("History", "win.history"),
            ("Downloads", "win.downloads"),
            ("Bookmarks", "win.bookmarks"),
            ("Bookmark This Page", "win.bookmark"),
        ]),
    );
    menu.append_section(
        None,
        &section(&[
            ("Hide Something", "win.hide"),
            ("What's Hidden Here", "win.hidden"),
            ("Reading Mode", "win.reader"),
            ("Find on Page", "win.find"),
        ]),
    );
    menu.append_section(
        None,
        &section(&[
            ("Tabs in Sidebar", "win.toggle-sidebar"),
            ("Settings", "win.settings"),
            ("Keyboard Shortcuts", "win.shortcuts"),
            ("Quit", "win.quit"),
        ]),
    );
    menu
}

#[cfg(test)]
mod memory_figure {
    use super::memory_text;

    #[test]
    fn writes_a_short_figure() {
        let small = memory_text(512);
        let mid = memory_text(128 * 1024);
        let big = memory_text(1536 * 1024);
        println!("{small}");
        println!("{mid}");
        println!("{big}");
        assert_eq!(small, "512 KB");
        assert_eq!(mid, "128 MB");
        assert_eq!(big, "1.5 GB");
    }
}
