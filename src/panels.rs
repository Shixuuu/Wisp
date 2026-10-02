//! The panels. All the same kind of thing, built the same way: a plate over
//! a lightly dimmed window, arriving on a spring and leaving quickly.

use crate::address;
use crate::browser::Browser;
use crate::motion::{Curve, Presence, Slide, Tween};
use crate::settings::{Engine, Look};
use adw::prelude::*;
use gtk::{gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use webkit6::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    History,
    Downloads,
    Bookmarks,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    General,
    Tabs,
    Shortcuts,
    Downloads,
    Privacy,
    About,
}

impl Page {
    const ALL: [Page; 6] = [Page::General, Page::Tabs, Page::Shortcuts, Page::Downloads, Page::Privacy, Page::About];

    fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Tabs => "Tabs",
            Page::Shortcuts => "Shortcuts",
            Page::Downloads => "Downloads",
            Page::Privacy => "Privacy",
            Page::About => "About",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::General => "preferences-system-symbolic",
            Page::Tabs => "view-dual-symbolic",
            Page::Shortcuts => "input-keyboard-symbolic",
            Page::Downloads => "folder-download-symbolic",
            Page::Privacy => "security-high-symbolic",
            Page::About => "help-about-symbolic",
        }
    }
}

pub struct Panels {
    b: Weak<Browser>,
    sheet: crate::motion::Place,
    holder: Slide,
    shown: Tween,
    pub open: Cell<Option<Panel>>,
    refill: RefCell<Option<Box<dyn Fn()>>>,
    page: Cell<Page>,
    clearing: Cell<bool>,
    hidden: Presence,
    hidden_catcher: crate::motion::Place,
    hidden_box: gtk::Box,
    popover: RefCell<Option<gtk::Popover>>,
}

impl Panels {
    pub fn new(b: &Rc<Browser>) -> Rc<Panels> {
        let dim = gtk::Box::new(gtk::Orientation::Vertical, 0);
        dim.add_css_class("dim-window");
        let holder_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let holder = Slide::new(&holder_box);
        holder.set_halign(gtk::Align::Center);
        holder.set_valign(gtk::Align::Center);
        let sheet = gtk::Overlay::new();
        sheet.set_child(Some(&dim));
        sheet.add_overlay(&holder);
        sheet.set_vexpand(true);
        let place = crate::motion::Place::new(&sheet);
        b.root.add_overlay(&place.root);
        let (h, d, p) = (holder.clone(), dim.clone(), place.clone());
        let shown = Tween::new(&sheet, 0.0, move |t| {
            if t <= 0.001 {
                p.settle();
            }
            d.set_opacity(t.clamp(0.0, 1.0));
            h.set_opacity(t.clamp(0.0, 1.0));
            h.set_scale(0.97 + 0.03 * t);
        });

        // The hidden things' list: no dimming, the page stays in view.
        let hidden_catcher = gtk::Box::new(gtk::Orientation::Vertical, 0);
        hidden_catcher.set_hexpand(true);
        hidden_catcher.set_vexpand(true);
        let hidden_catcher = crate::motion::Place::new(&hidden_catcher);
        b.root.add_overlay(&hidden_catcher.root);
        let hidden_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let hidden = Presence::new(&hidden_box, 0.97, 0.0);
        hidden.slide.set_anchor(1.0, 0.0);
        hidden.root.set_halign(gtk::Align::End);
        hidden.root.set_valign(gtk::Align::Start);
        hidden.root.set_margin_end(14);
        b.root.add_overlay(&hidden.root);

        let panels = Rc::new(Panels {
            b: b.weak(),
            sheet: place.clone(),
            holder,
            shown,
            open: Cell::new(None),
            refill: RefCell::default(),
            page: Cell::new(Page::General),
            clearing: Cell::new(false),
            hidden,
            hidden_catcher,
            hidden_box,
            popover: RefCell::default(),
        });
        let click = gtk::GestureClick::new();
        let me = Rc::downgrade(&panels);
        click.connect_released(move |_, _, _, _| {
            if let Some(me) = me.upgrade() {
                me.close();
            }
        });
        dim.add_controller(click);
        let click = gtk::GestureClick::new();
        let me = Rc::downgrade(&panels);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(me) = me.upgrade() {
                me.show_hidden(false);
            }
        });
        panels.hidden_catcher.root.add_controller(click);
        panels
    }

    fn browser(&self) -> Option<Rc<Browser>> {
        self.b.upgrade()
    }

    pub fn toggle(self: &Rc<Self>, panel: Panel) {
        if self.open.get() == Some(panel) {
            self.close();
        } else {
            self.show(panel);
        }
    }

    pub fn show(self: &Rc<Self>, panel: Panel) {
        let Some(b) = self.browser() else { return };
        if self.hidden_showing() {
            self.show_hidden(false);
        }
        self.clearing.set(false);
        self.open.set(Some(panel));
        self.refill.take();
        let content = match panel {
            Panel::History => self.history(&b),
            Panel::Downloads => self.downloads(&b),
            Panel::Bookmarks => self.bookmarks(&b),
            Panel::Settings => self.settings(&b),
        };
        let holder = self.holder.child().and_downcast::<gtk::Box>().expect("holder is a box");
        while let Some(c) = holder.first_child() {
            holder.remove(&c);
        }
        holder.append(&content);
        self.sheet.show(true);
        self.shown.to(1.0, Curve::Settle);
    }

    pub fn show_settings(self: &Rc<Self>, page: Page) {
        self.page.set(page);
        self.show(Panel::Settings);
    }

    /// Escape and Ctrl+W close the front panel. True when one was open.
    pub fn close(&self) -> bool {
        if self.open.take().is_none() {
            return false;
        }
        self.clearing.set(false);
        self.refill.take();
        self.sheet.release();
        self.shown.to(0.0, Curve::Quick);
        if let Some(v) = self.browser().and_then(|b| b.active()).and_then(|t| t.view.borrow().clone()) {
            v.grab_focus();
        }
        true
    }

    fn refill(&self) {
        if let Some(f) = self.refill.borrow().as_ref() {
            f();
        }
    }

    pub fn refresh_downloads(&self) {
        if self.open.get() == Some(Panel::Downloads) {
            self.refill();
        }
    }

    // MARK: history

    fn history(self: &Rc<Self>, b: &Rc<Browser>) -> gtk::Widget {
        let (hunt, query) = hunt("Search everywhere you have been");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let scroller = scroll(&list, 420);
        let count = gtk::Label::new(None);
        count.add_css_class("foot-text");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
        content.append(&hunt);
        content.append(&scroller);
        let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let foot_view = foot.clone();
        let weak = b.weak();
        let me = Rc::downgrade(self);
        let query_weak = query.downgrade();
        let fill = move || {
            let Some(q) = query_weak.upgrade() else { return };
            let (Some(b), Some(me)) = (weak.upgrade(), me.upgrade()) else { return };
            clear(&list);
            clear(&foot);
            if me.clearing.get() {
                list.append(&me.sweeps(&b));
                let back = pill("Back", false);
                let me2 = Rc::downgrade(&me);
                back.connect_clicked(move |_| {
                    if let Some(me) = me2.upgrade() {
                        me.clearing.set(false);
                        me.refill();
                    }
                });
                foot.append(&spacer());
                foot.append(&back);
                return;
            }
            let traces = b.history.borrow().everything(&q.text());
            let shown = traces.len().min(500);
            count.set_label(&if traces.len() <= 500 {
                if traces.len() == 1 { "1 page".into() } else { format!("{} pages", traces.len()) }
            } else {
                format!("{shown} of {} pages", traces.len())
            });
            foot.append(&count);
            foot.append(&spacer());
            let clear_pill = pill("Clear…", false);
            let me2 = Rc::downgrade(&me);
            clear_pill.connect_clicked(move |_| {
                if let Some(me) = me2.upgrade() {
                    me.clearing.set(true);
                    me.refill();
                }
            });
            foot.append(&clear_pill);
            if traces.is_empty() {
                list.append(&nothing(if q.text().is_empty() { "Nothing yet." } else { "Nothing matches." }));
                return;
            }
            let mut day = String::new();
            let mut card = card();
            for (i, t) in traces.iter().take(500).enumerate() {
                let this_day = day_of(t.last);
                if this_day != day {
                    if i > 0 {
                        list.append(&card);
                        card = self::card();
                    }
                    let c = caption(&this_day);
                    c.set_margin_top(if i == 0 { 0 } else { 14 });
                    c.set_margin_bottom(6);
                    list.append(&c);
                    day = this_day;
                } else {
                    card.append(&rule(14));
                }
                let title = if t.title.is_empty() { address::pretty(&t.url) } else { t.title.clone() };
                let (weak, url) = (b.weak(), t.url.clone());
                let (weak2, url2, me3) = (b.weak(), t.url.clone(), Rc::downgrade(&me));
                card.append(&trace_row(
                    &t.url,
                    &title,
                    &address::pretty(&t.url),
                    &clock(t.last),
                    move || {
                        if let Some(b) = weak.upgrade() {
                            b.ui().panels.close();
                            b.go(&url);
                        }
                    },
                    move || {
                        if let Some(b) = weak2.upgrade() {
                            b.history.borrow_mut().forget(&url2);
                        }
                        if let Some(me) = me3.upgrade() {
                            me.refill();
                        }
                    },
                ));
            }
            list.append(&card);
        };
        fill();
        let fill = Rc::new(fill);
        let f = fill.clone();
        query.connect_changed(move |_| f());
        *self.refill.borrow_mut() = Some(Box::new(move || fill()));
        focus_later(&query);
        plate(self, "History", 600, &content, Some(&foot_view))
    }

    fn sweeps(self: &Rc<Self>, b: &Rc<Browser>) -> gtk::Widget {
        let me = Rc::downgrade(self);
        remembered(b, "Everywhere you have been", move || {
            if let Some(me) = me.upgrade() {
                me.clearing.set(false);
                me.refill();
            }
        })
        .upcast()
    }

    // MARK: downloads

    fn downloads(self: &Rc<Self>, b: &Rc<Browser>) -> gtk::Widget {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&scroll(&list, 420));
        let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let foot_view = foot.clone();
        let weak = b.weak();
        let fill = move || {
            let Some(b) = weak.upgrade() else { return };
            clear(&list);
            clear(&foot);
            let fetches = b.fetches.borrow();
            let kept = b.loot.borrow().kept.clone();
            if fetches.is_empty() && kept.is_empty() {
                list.append(&nothing("Nothing downloaded yet."));
            }
            if !fetches.is_empty() {
                let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
                section.append(&caption("Current downloads"));
                let c = card();
                for (i, f) in fetches.iter().enumerate() {
                    if i > 0 {
                        c.append(&rule(14));
                    }
                    c.append(&fetch_row(f));
                }
                section.append(&c);
                list.append(&section);
            }
            drop(fetches);
            if !kept.is_empty() {
                let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
                section.append(&caption("Completed"));
                let c = card();
                for (i, k) in kept.iter().enumerate() {
                    if i > 0 {
                        c.append(&rule(14));
                    }
                    c.append(&kept_row(&b, k));
                }
                section.append(&c);
                list.append(&section);
            }
            let dir = crate::store::downloads_dir();
            let text = gtk::Label::new(Some(&if kept.is_empty() {
                format!(
                    "Files land in {}",
                    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                )
            } else {
                "Clearing the list leaves the files where they are".into()
            }));
            text.add_css_class("foot-text");
            foot.append(&text);
            foot.append(&spacer());
            if !kept.is_empty() {
                let clear_list = pill("Clear list", false);
                let weak = b.weak();
                clear_list.connect_clicked(move |_| {
                    if let Some(b) = weak.upgrade() {
                        b.loot.borrow_mut().forget_all();
                        b.ui().panels.refill();
                    }
                });
                foot.append(&clear_list);
            }
        };
        fill();
        *self.refill.borrow_mut() = Some(Box::new(fill));
        plate(self, "Downloads", 560, &content, Some(&foot_view))
    }

    // MARK: bookmarks

    fn bookmarks(self: &Rc<Self>, b: &Rc<Browser>) -> gtk::Widget {
        let (hunt, query) = hunt("Search bookmarks");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
        content.append(&hunt);
        content.append(&scroll(&list, 440));
        let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let foot_view = foot.clone();
        let weak = b.weak();
        let query_weak = query.downgrade();
        let fill = move || {
            let Some(q) = query_weak.upgrade() else { return };
            let Some(b) = weak.upgrade() else { return };
            clear(&list);
            clear(&foot);
            let marks = b.bookmarks.borrow();
            let text = q.text().to_string();
            if marks.roots.is_empty() {
                list.append(&nothing("Nothing kept yet. Add this page with Ctrl+Shift+B, or bring yours in below."));
            } else if !text.is_empty() {
                let hits = marks.matches(&text);
                if hits.is_empty() {
                    list.append(&nothing("Nothing matches."));
                } else {
                    let c = card();
                    for (i, (node, path)) in hits.iter().enumerate() {
                        if i > 0 {
                            c.append(&rule(40));
                        }
                        let folders = if path.is_empty() { "Top level".to_string() } else { path.join(" › ") };
                        let whereabouts = match node.url.as_deref().and_then(address::bare_host) {
                            Some(host) => format!("{folders}  ·  {host}"),
                            None => folders,
                        };
                        if node.url.is_none() {
                            continue;
                        }
                        let (weak, url) = (b.weak(), node.url.clone());
                        c.append(&trace_row(
                            node.url.as_deref().unwrap_or(""),
                            &node.title,
                            &whereabouts,
                            "",
                            move || {
                                if let (Some(b), Some(url)) = (weak.upgrade(), url.clone()) {
                                    b.ui().panels.close();
                                    b.go(&url);
                                }
                            },
                            || {},
                        ));
                    }
                    list.append(&c);
                }
            } else {
                let c = card();
                outline(&b, &marks.roots, &c, 0);
                list.append(&c);
            }
            let count = crate::bookmarks::Bookmarks::count(&marks.roots);
            drop(marks);
            let from = gtk::Label::new(Some("Bring in from"));
            from.add_css_class("foot-text");
            foot.append(&from);
            let bring = pill("Bring in…", false);
            let weak2 = b.weak();
            bring.connect_clicked(move |_| {
                if let Some(b) = weak2.upgrade() {
                    bring_in(&b);
                }
            });
            foot.append(&bring);
            foot.append(&spacer());
            let folder = pill("New Folder…", false);
            let weak2 = b.weak();
            folder.connect_clicked(move |_| {
                if let Some(b) = weak2.upgrade() {
                    ask_name(&b, "New Folder", move |b, name| {
                        b.bookmarks.borrow_mut().new_folder(name);
                        b.ui().panels.refill();
                    });
                }
            });
            foot.append(&folder);
            let n = gtk::Label::new(Some(&if count == 1 { "1 bookmark".into() } else { format!("{count} bookmarks") }));
            n.add_css_class("foot-text");
            foot.append(&n);
        };
        fill();
        let fill = Rc::new(fill);
        let f = fill.clone();
        query.connect_changed(move |_| f());
        let weak = b.weak();
        query.connect_activate(move |q| {
            let Some(b) = weak.upgrade() else { return };
            if q.text().trim().is_empty() {
                return;
            }
            let first = b.bookmarks.borrow().matches(&q.text()).into_iter().find_map(|(n, _)| n.url);
            if let Some(url) = first {
                b.ui().panels.close();
                b.go(&url);
            }
        });
        *self.refill.borrow_mut() = Some(Box::new(move || fill()));
        focus_later(&query);
        plate(self, "Bookmarks", 600, &content, Some(&foot_view))
    }

    /// The bookmarks button: the list, and a way to keep this page.
    pub fn bookmarks_dropdown(self: &Rc<Self>, anchor: &gtk::Widget) {
        let Some(b) = self.browser() else { return };
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("dropdown");
        let marks = b.bookmarks.borrow();
        if marks.roots.is_empty() {
            let none = gtk::Label::new(Some("No bookmarks yet"));
            none.add_css_class("muted-text");
            none.set_margin_top(14);
            none.set_margin_bottom(14);
            none.set_margin_start(14);
            none.set_xalign(0.0);
            body.append(&none);
        } else {
            let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
            list.set_margin_top(6);
            list.set_margin_bottom(6);
            list.set_margin_start(6);
            list.set_margin_end(6);
            outline(&b, &marks.roots, &list, 0);
            body.append(&scroll(&list, 360));
        }
        drop(marks);
        body.append(&rule(0));
        let kept = b.active().is_some_and(|t| !t.is_blank() && b.bookmarks.borrow().find(&t.address()).is_some());
        let foot = gtk::Box::new(gtk::Orientation::Vertical, 1);
        foot.set_margin_top(6);
        foot.set_margin_bottom(6);
        foot.set_margin_start(6);
        foot.set_margin_end(6);
        let add = foot_row(if kept { "Edit This Bookmark…" } else { "Add This Page" });
        add.set_action_name(Some("win.bookmark"));
        let manage = foot_row("Manage Bookmarks…");
        manage.set_action_name(Some("win.bookmarks"));
        foot.append(&add);
        foot.append(&manage);
        body.append(&foot);
        self.pop(anchor, &body);
    }

    /// Ctrl+Shift+B: the page kept, and a card to name it and file it.
    pub fn bookmark_card(self: &Rc<Self>, anchor: Option<&gtk::Widget>) {
        let Some(b) = self.browser() else { return };
        let Some(tab) = b.active().filter(|t| !t.is_blank()) else { return };
        let (mark, fresh) = b.bookmarks.borrow_mut().keep(&tab.address(), &tab.label());
        b.refresh_tabs();
        let Some(anchor) = anchor.filter(|a| a.is_mapped()) else {
            b.announce(if fresh { "Bookmarked" } else { "Already a bookmark" });
            return;
        };
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.add_css_class("dropdown");
        body.add_css_class("card-pop");
        let head = gtk::Label::new(Some("Bookmarked"));
        head.add_css_class("card-title");
        head.set_xalign(0.0);
        body.append(&head);
        let name = crate::text_box("Bookmark name");
        name.set_text(&mark.title);
        name.add_css_class("wash-field");
        name.set_hexpand(true);
        let id = mark.id;
        let weak = b.weak();
        name.connect_changed(move |n| {
            let typed = n.text().trim().to_string();
            if let (Some(b), false) = (weak.upgrade(), typed.is_empty()) {
                b.bookmarks.borrow_mut().rename(id, &typed);
            }
        });
        let me = Rc::downgrade(self);
        name.connect_activate(move |_| {
            if let Some(p) = me.upgrade().and_then(|m| m.popover.borrow().clone()) {
                p.popdown();
            }
        });
        body.append(&labelled("Name", &name));
        let folder = gtk::MenuButton::new();
        folder.add_css_class("wash-field");
        folder.set_label(&b.bookmarks.borrow().folder_of(id).unwrap_or_else(|| "Top Level".into()));
        let menu = gio::Menu::new();
        let group = gio::SimpleActionGroup::new();
        let file = gio::SimpleAction::new("file", Some(glib::VariantTy::INT64));
        let weak = b.weak();
        let f = folder.clone();
        file.connect_activate(move |_, v| {
            let Some(b) = weak.upgrade() else { return };
            let target = v.and_then(|v| v.get::<i64>()).filter(|&t| t >= 0).map(|t| t as u64);
            b.bookmarks.borrow_mut().move_into(id, target);
            f.set_label(&b.bookmarks.borrow().folder_of(id).unwrap_or_else(|| "Top Level".into()));
        });
        group.add_action(&file);
        folder.insert_action_group("card", Some(&group));
        let top = gio::MenuItem::new(Some("Top Level"), None);
        top.set_action_and_target_value(Some("card.file"), Some(&(-1i64).to_variant()));
        menu.append_item(&top);
        for (fid, title, depth) in b.bookmarks.borrow().folders() {
            let item = gio::MenuItem::new(Some(&format!("{}{title}", "   ".repeat(depth))), None);
            item.set_action_and_target_value(Some("card.file"), Some(&(fid as i64).to_variant()));
            menu.append_item(&item);
        }
        folder.set_menu_model(Some(&menu));
        body.append(&labelled("Folder", &folder));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let remove = quick("Remove", true);
        let weak = b.weak();
        let me = Rc::downgrade(self);
        remove.connect_clicked(move |_| {
            if let Some(b) = weak.upgrade() {
                b.bookmarks.borrow_mut().remove(id);
                b.refresh_tabs();
            }
            if let Some(p) = me.upgrade().and_then(|m| m.popover.borrow().clone()) {
                p.popdown();
            }
        });
        row.append(&remove);
        row.append(&spacer());
        let done = pill("Done", true);
        let me = Rc::downgrade(self);
        done.connect_clicked(move |_| {
            if let Some(p) = me.upgrade().and_then(|m| m.popover.borrow().clone()) {
                p.popdown();
            }
        });
        row.append(&done);
        body.append(&row);
        self.pop(anchor, &body);
        focus_later(&name);
    }

    fn pop(&self, anchor: &gtk::Widget, body: &gtk::Box) {
        if let Some(old) = self.popover.take() {
            old.popdown();
        }
        let popover = gtk::Popover::new();
        popover.add_css_class("wisp-pop");
        popover.set_has_arrow(false);
        popover.set_child(Some(body));
        popover.set_parent(anchor);
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
        *self.popover.borrow_mut() = Some(popover);
    }

    // MARK: hidden things

    pub fn hidden_showing(&self) -> bool {
        self.hidden.shown()
    }

    pub fn show_hidden(&self, on: bool) {
        self.hidden_catcher.show(on);
        self.hidden.show(on);
        if on {
            self.refresh_hidden();
        } else if let Some(b) = self.browser()
            && let Some(tab) = b.active()
        {
            b.tune(&tab, &tab.address(), None);
            tab.js("document.getElementById('wisp-peek')?.remove()");
        }
    }

    pub fn refresh_hidden(&self) {
        let Some(b) = self.browser() else { return };
        if !self.hidden.shown() {
            return;
        }
        let strip = if b.prefs.borrow().sidebar { 8 } else { crate::browser::STRIP as i32 + 8 };
        self.hidden.root.set_margin_top(strip);
        clear(&self.hidden_box);
        let Some(tab) = b.active() else { return };
        let host = tab.host();
        let veils = host.as_ref().map(|h| b.curtain.borrow().veils(h).to_vec()).unwrap_or_default();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        if veils.is_empty() {
            content.append(&nothing("Nothing is hidden here."));
        } else {
            content.append(&caption("Hidden on this site — rest on a line to see it"));
            let c = card();
            for (i, v) in veils.iter().enumerate() {
                if i > 0 {
                    c.append(&rule(14));
                }
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                row.add_css_class("list-row");
                let text = two_lines(&v.label, &v.note);
                text.set_hexpand(true);
                row.append(&text);
                let restore = quick("Restore", false);
                restore.set_opacity(0.0);
                row.append(&restore);
                let hover = gtk::EventControllerMotion::new();
                let (weak, sel, r) = (b.weak(), v.selector.clone(), restore.clone());
                hover.connect_enter(move |_, _, _| {
                    r.set_opacity(1.0);
                    if let Some(b) = weak.upgrade()
                        && let Some(tab) = b.active() {
                            // Shown with the layout it had, outlined, in view.
                            b.tune(&tab, &tab.address(), Some(&sel));
                            let q = serde_json::to_string(&sel).unwrap_or_default();
                            tab.js(&format!(
                                "(function(){{var s=document.getElementById('wisp-peek')||document.head.appendChild(document.createElement('style'));s.id='wisp-peek';s.textContent={q}+' {{ outline: 2px solid rgba(23,23,23,.9) !important; outline-offset: 2px !important; }}';try{{document.querySelector({q}).scrollIntoView({{block:'center',behavior:'smooth'}})}}catch(e){{}}}})()"
                            ));
                        }
                });
                let (r, weak) = (restore.clone(), b.weak());
                hover.connect_leave(move |_| {
                    r.set_opacity(0.0);
                    if let Some(b) = weak.upgrade()
                        && let Some(tab) = b.active()
                    {
                        b.tune(&tab, &tab.address(), None);
                        tab.js("document.getElementById('wisp-peek')?.remove()");
                    }
                });
                row.add_controller(hover);
                let (weak, sel, h) = (b.weak(), v.selector.clone(), host.clone().unwrap_or_default());
                restore.connect_clicked(move |_| {
                    let Some(b) = weak.upgrade() else { return };
                    b.curtain.borrow_mut().restore(&h, &sel);
                    b.retune();
                    b.ui().panels.refresh_hidden();
                });
                c.append(&row);
            }
            content.append(&scroll(&c, 320));
        }
        let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let hide = pill("Hide something…", true);
        let weak = b.weak();
        hide.connect_clicked(move |_| {
            if let Some(b) = weak.upgrade() {
                b.ui().panels.show_hidden(false);
                b.toggle_hiding();
            }
        });
        foot.append(&hide);
        if !veils.is_empty() {
            let all = pill("Restore all", false);
            let (weak, h) = (b.weak(), host.clone().unwrap_or_default());
            all.connect_clicked(move |_| {
                let Some(b) = weak.upgrade() else { return };
                b.curtain.borrow_mut().restore_all(&h);
                b.retune();
                b.ui().panels.refresh_hidden();
            });
            foot.append(&all);
        }
        let title = host.unwrap_or_else(|| "This page".into());
        let weak = b.weak();
        self.hidden_box.append(&plate_of(&title, 380, &content, Some(foot.upcast_ref()), move || {
            if let Some(b) = weak.upgrade() {
                b.ui().panels.show_hidden(false);
            }
        }));
    }

    // MARK: settings

    fn settings(self: &Rc<Self>, b: &Rc<Browser>) -> gtk::Widget {
        let rail = gtk::Box::new(gtk::Orientation::Vertical, 2);
        rail.add_css_class("rail");
        let heading = gtk::Label::new(Some("Settings"));
        heading.add_css_class("rail-title");
        heading.set_xalign(0.0);
        rail.append(&heading);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("settings-body");
        body.set_hexpand(true);
        let title = gtk::Label::new(None);
        title.add_css_class("plate-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let close = crate::tabs::door("window-close-symbolic", "Done   esc");
        let me = Rc::downgrade(self);
        close.connect_clicked(move |_| {
            if let Some(me) = me.upgrade() {
                me.close();
            }
        });
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        head.append(&title);
        head.append(&close);
        head.set_margin_bottom(16);
        let pages = gtk::Box::new(gtk::Orientation::Vertical, 18);
        body.append(&head);
        body.append(&scroll(&pages, 420));
        let rows: Rc<RefCell<Vec<(Page, gtk::Button)>>> = Rc::default();
        let weak = b.weak();
        let me = Rc::downgrade(self);
        let (t, p) = (title.clone(), pages.clone());
        let rows_weak = Rc::downgrade(&rows);
        let show: Rc<dyn Fn(Page)> = Rc::new(move |page| {
            let (Some(b), Some(me), Some(r)) = (weak.upgrade(), me.upgrade(), rows_weak.upgrade()) else { return };
            me.page.set(page);
            t.set_label(page.title());
            clear(&p);
            for (pg, row) in r.borrow().iter() {
                if *pg == page { row.add_css_class("on") } else { row.remove_css_class("on") }
            }
            match page {
                Page::General => general(&b, &p),
                Page::Tabs => tabs(&b, &p),
                Page::Shortcuts => shortcuts(&p),
                Page::Downloads => downloads(&b, &p),
                Page::Privacy => privacy(&b, &p),
                Page::About => about(&p),
            }
        });
        for page in Page::ALL {
            let row = gtk::Button::new();
            row.add_css_class("rail-row");
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 9);
            let icon = gtk::Image::from_icon_name(page.icon());
            icon.set_pixel_size(14);
            inner.append(&icon);
            inner.append(&gtk::Label::new(Some(page.title())));
            row.set_child(Some(&inner));
            let s = show.clone();
            row.connect_clicked(move |_| s(page));
            rail.append(&row);
            rows.borrow_mut().push((page, row));
        }
        let keeper = rows.clone();
        rail.connect_realize(move |_| {
            let _ = &keeper;
        });
        show(self.page.get());
        let me = Rc::downgrade(self);
        let s = show.clone();
        *self.refill.borrow_mut() = Some(Box::new(move || {
            if let Some(me) = me.upgrade() {
                s(me.page.get());
            }
        }));
        let frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        frame.add_css_class("plate");
        frame.add_css_class("settings");
        frame.set_size_request(660, 500);
        frame.append(&rail);
        frame.append(&body);
        frame.upcast()
    }
}

// MARK: the settings pages

fn toggle(b: &Rc<Browser>, on: bool, set: impl Fn(&Rc<Browser>, bool) + 'static) -> gtk::Widget {
    let weak = b.weak();
    switch(on, move |on| {
        if let Some(b) = weak.upgrade() {
            set(&b, on);
            b.prefs.borrow().save();
        }
    })
}

fn general(b: &Rc<Browser>, page: &gtk::Box) {
    let c = card();
    let default = pill("Make default", true);
    default.connect_clicked(|button| {
        let launched = gio::Subprocess::newv(
            &[
                "xdg-settings".as_ref(),
                "set".as_ref(),
                "default-web-browser".as_ref(),
                "org.wisp.Wisp.desktop".as_ref(),
            ],
            gio::SubprocessFlags::NONE,
        );
        button.set_label(if launched.is_ok() { "Asked" } else { "xdg-settings is missing" });
    });
    c.append(&line("Open links from other apps", Some("Make Wisp the browser other programs open links in"), &default));
    c.append(&rule(14));
    let bring = pill("Bring in", false);
    let weak = b.weak();
    bring.connect_clicked(move |_| {
        if let Some(b) = weak.upgrade() {
            bring_in(&b);
        }
    });
    c.append(&line(
        "Bring things over",
        Some("Bookmarks from Chromium, Chrome, Brave, Vivaldi or Edge on this computer"),
        &bring,
    ));
    c.append(&rule(14));
    let engines = gtk::MenuButton::new();
    engines.add_css_class("pill");
    engines.set_label(b.prefs.borrow().engine.name());
    let menu = gio::Menu::new();
    let group = gio::SimpleActionGroup::new();
    let pick = gio::SimpleAction::new("engine", Some(glib::VariantTy::INT32));
    let weak = b.weak();
    let e = engines.clone();
    pick.connect_activate(move |_, v| {
        let Some(b) = weak.upgrade() else { return };
        let engine = Engine::ALL[v.and_then(|v| v.get::<i32>()).unwrap_or(0) as usize];
        b.prefs.borrow_mut().engine = engine;
        b.prefs.borrow().save();
        e.set_label(engine.name());
        b.ui().panels.refill();
    });
    group.add_action(&pick);
    engines.insert_action_group("settings", Some(&group));
    for (i, engine) in Engine::ALL.iter().enumerate() {
        let item = gio::MenuItem::new(Some(engine.name()), None);
        item.set_action_and_target_value(Some("settings.engine"), Some(&(i as i32).to_variant()));
        menu.append_item(&item);
    }
    engines.set_menu_model(Some(&menu));
    c.append(&line("Search with", Some("What words typed in the address field are sent to"), &engines));
    if b.prefs.borrow().engine == Engine::Custom {
        let custom = crate::text_box("Search address");
        custom.add_css_class("wash-field");
        custom.set_text(&b.prefs.borrow().custom_engine);
        custom.set_placeholder_text(Some("https://example.com/search?q=%s"));
        custom.set_width_chars(28);
        let weak = b.weak();
        custom.connect_changed(move |t| {
            if let Some(b) = weak.upgrade() {
                b.prefs.borrow_mut().custom_engine = t.text().to_string();
            }
        });
        let weak = b.weak();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| {
            if let Some(b) = weak.upgrade() {
                b.prefs.borrow().save();
            }
        });
        custom.add_controller(focus);
        c.append(&rule(14));
        c.append(&line("Custom search", Some("An address with %s where the words go"), &custom));
    }
    c.append(&rule(14));
    let words: Vec<String> = b.prefs.borrow().keywords.iter().map(|k| k.keyword.clone()).collect();
    c.append(&line(
        "Site shortcuts",
        Some(&format!(
            "A word, a space, then what to search: {}. Kept in ~/.config/wisp/settings.json",
            words.join(", ")
        )),
        &gtk::Box::new(gtk::Orientation::Horizontal, 0),
    ));
    page.append(&c);

    let c = card();
    let weak = b.weak();
    let look = b.prefs.borrow().look;
    c.append(&line(
        "Appearance",
        Some("Light, dark, or whatever the desktop is doing — pages follow it too"),
        &segmented(&[(Look::Light, "Light"), (Look::Dark, "Dark"), (Look::System, "System")], look, move |l| {
            if let Some(b) = weak.upgrade() {
                b.set_look(l);
            }
        }),
    ));
    c.append(&rule(14));
    let weak = b.weak();
    c.append(&line(
        "Page zoom",
        Some("Where every site starts. Ctrl+plus and Ctrl+minus are still remembered for each site."),
        &steps(
            &[0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0],
            b.prefs.borrow().page_zoom,
            1.0,
            move |z| {
                if let Some(b) = weak.upgrade() {
                    b.prefs.borrow_mut().page_zoom = z;
                    b.prefs.borrow().save();
                }
            },
        ),
    ));
    c.append(&rule(14));
    c.append(&line(
        "Address bar commands",
        Some("A word like \"settings\" or \"new tab\", typed alone in the address field, goes there instead of searching for it"),
        &toggle(b, b.prefs.borrow().command_bar, |b, on| b.prefs.borrow_mut().command_bar = on),
    ));
    c.append(&rule(14));
    c.append(&line(
        "Show where links go",
        Some("Point at a link and its address shows at the bottom of the page"),
        &toggle(b, b.prefs.borrow().shows_links, |b, on| b.prefs.borrow_mut().shows_links = on),
    ));
    c.append(&rule(14));
    c.append(&line(
        "Draw pages on the graphics card",
        Some("Faster scrolling and video. Turn off if a driver shows blank or flickering pages. New pages follow"),
        &toggle(b, b.prefs.borrow().hardware_acceleration, |b, on| {
            b.prefs.borrow_mut().hardware_acceleration = on;
            crate::web::apply(&b.web.settings, &b.prefs.borrow());
        }),
    ));
    page.append(&c);
}

fn tabs(b: &Rc<Browser>, page: &gtk::Box) {
    let c = card();
    let side = b.prefs.borrow().sidebar;
    c.append(&line(
        "Tabs in a sidebar",
        Some(
            "Down the side instead of across the top. Pull its edge to make it wider; double-click the edge to reset.",
        ),
        &toggle(b, side, |b, _| {
            b.toggle_sidebar();
            b.ui().panels.refill();
        }),
    ));
    if side {
        c.append(&rule(14));
        let weak = b.weak();
        let right = b.prefs.borrow().side_right;
        c.append(&line(
            "Sidebar position",
            Some("Which edge of the window the tabs run down"),
            &segmented(&[(false, "Left"), (true, "Right")], right, move |r| {
                if let Some(b) = weak.upgrade() {
                    b.prefs.borrow_mut().side_right = r;
                    b.prefs.borrow().save();
                    b.build_chrome();
                }
            }),
        ));
        c.append(&rule(14));
        c.append(&line(
            "Hide the sidebar until the pointer reaches the edge",
            Some("The page takes the whole window; push against its edge for the tabs. Ctrl+S keeps them out."),
            &toggle(b, b.prefs.borrow().side_hides, |b, on| {
                b.prefs.borrow_mut().side_hides = on;
                b.folded.set(on);
                b.peeking.set(false);
                b.arrange(true);
            }),
        ));
    } else {
        c.append(&rule(14));
        c.append(&line(
            "Back, forward and reload on the left",
            Some("Beside the window's buttons, before the tabs"),
            &toggle(b, b.prefs.borrow().navigation_left, |b, on| {
                b.prefs.borrow_mut().navigation_left = on;
                b.build_chrome();
            }),
        ));
    }
    c.append(&rule(14));
    c.append(&line(
        "Show how far you've read",
        Some("The tab you're on fills with grey as you scroll down the page"),
        &toggle(b, b.prefs.borrow().shows_reading, |b, on| {
            b.prefs.borrow_mut().shows_reading = on;
            b.refresh_tabs();
        }),
    ));
    c.append(&rule(14));
    c.append(&line(
        "Sleep tabs you aren't using",
        Some("After half an hour away they come back where you left them. Pinned tabs and sound stay awake."),
        &toggle(b, b.prefs.borrow().sleeps_tabs, |b, on| b.prefs.borrow_mut().sleeps_tabs = on),
    ));
    c.append(&rule(14));
    c.append(&line(
        "Load background tabs when you go to them",
        Some("A link opened behind the page, with Ctrl-click or the middle button, waits until you go to its tab."),
        &toggle(b, b.prefs.borrow().lazy_tabs, |b, on| b.prefs.borrow_mut().lazy_tabs = on),
    ));
    c.append(&rule(14));
    c.append(&line(
        "Start with a fresh window",
        Some("Each time Wisp opens, your pinned tabs are there and last time's other tabs aren't."),
        &toggle(b, b.prefs.borrow().starts_fresh, |b, on| b.prefs.borrow_mut().starts_fresh = on),
    ));
    page.append(&c);
}

const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Tabs",
        &[
            ("New tab", "Ctrl+T"),
            ("New private tab", "Ctrl+Shift+N"),
            ("Close tab or panel", "Ctrl+W"),
            ("Reopen closed tab", "Ctrl+Shift+T"),
            ("Duplicate tab", "Ctrl+D"),
            ("Switch to an open page", "Ctrl+K"),
            ("Most recent tabs", "Ctrl+Tab"),
            ("Previous, next tab", "Ctrl+Shift+[  Ctrl+Shift+]"),
            ("Jump to a tab", "Ctrl+1 … Ctrl+9"),
            ("Tabs across the top or down the side", "Ctrl+Shift+S"),
            ("Fold the tabs away", "Ctrl+S"),
        ],
    ),
    (
        "Page",
        &[
            ("Address", "Ctrl+L"),
            ("Back, forward", "Ctrl+[  Ctrl+]"),
            ("Reload", "Ctrl+R"),
            ("Reload from the server", "Ctrl+Alt+R"),
            ("Reading mode", "Ctrl+Shift+R"),
            ("Find, next, previous", "Ctrl+F  Ctrl+G  Shift+Enter"),
            ("Pin this tab", "Ctrl+Shift+P"),
            ("Group this tab", "Ctrl+Shift+G"),
            ("Zoom in, out, actual size", "Ctrl++  Ctrl+−  Ctrl+0"),
            ("Copy address", "Ctrl+Shift+C"),
            ("Paste and go", "Ctrl+Shift+V"),
            ("Pause what's playing", "Ctrl+Shift+M"),
            ("Print", "Ctrl+P"),
        ],
    ),
    (
        "Keeping and hiding",
        &[
            ("Bookmark this page", "Ctrl+Shift+B"),
            ("Hide something", "Ctrl+Shift+H"),
            ("What's hidden here", "Ctrl+Shift+U"),
            ("Undo a hide, while hiding", "Ctrl+Z"),
        ],
    ),
    (
        "Panels",
        &[("History", "Ctrl+Y"), ("Downloads", "Ctrl+Shift+J"), ("Settings", "Ctrl+,"), ("Web inspector", "F12")],
    ),
];

fn shortcuts(page: &gtk::Box) {
    for (title, keys) in SHORTCUTS {
        let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
        section.append(&caption(title));
        let c = card();
        for (i, (what, key)) in keys.iter().enumerate() {
            if i > 0 {
                c.append(&rule(14));
            }
            let k = gtk::Label::new(Some(key));
            k.add_css_class("keybox");
            c.append(&line(what, None, &k));
        }
        section.append(&c);
        page.append(&section);
    }
}

fn downloads(b: &Rc<Browser>, page: &gtk::Box) {
    let c = card();
    let dir = crate::store::downloads_dir();
    let home = std::env::var("HOME").unwrap_or_default();
    let raw = dir.to_string_lossy();
    let shown = if home.is_empty() { raw.into_owned() } else { raw.replacen(&home, "~", 1) };
    let open = pill("Show", false);
    let d = dir.clone();
    open.connect_clicked(move |_| {
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&d)));
        launcher.launch(None::<&gtk::Window>, None::<&gio::Cancellable>, |_| {});
    });
    c.append(&line("Save to", Some(&shown), &open));
    c.append(&rule(14));
    c.append(&line(
        "Always show the downloads button",
        Some("Beside the other buttons, even with nothing downloading. Off, it shows only while a file comes in"),
        &toggle(b, b.prefs.borrow().always_shows_downloads, |b, on| {
            b.prefs.borrow_mut().always_shows_downloads = on;
            b.fetches_changed();
        }),
    ));
    page.append(&c);
}

fn privacy(b: &Rc<Browser>, page: &gtk::Box) {
    let c = card();
    c.append(&line(
        "Block ads and trackers",
        Some("Third parties whose only job is to watch"),
        &toggle(b, b.prefs.borrow().shielded, |b, on| {
            b.prefs.borrow_mut().shielded = on;
            b.retune();
        }),
    ));
    if let Some(host) = b.active().and_then(|t| t.host()) {
        c.append(&rule(14));
        let paused = b.prefs.borrow().is_paused(&host);
        let h = host.clone();
        c.append(&line(
            &format!("Block on {host}"),
            Some("Turn off here if the site breaks — the page reloads"),
            &toggle(b, !paused, move |b, on| {
                b.prefs.borrow_mut().set_paused(&h, !on);
                b.retune();
                if let Some(v) = b.active().and_then(|t| t.view.borrow().clone()) {
                    v.reload();
                }
            }),
        ));
    }
    c.append(&rule(14));
    c.append(&line(
        "Prevent cross-site tracking",
        Some(
            "As in Safari. Off, trackers inside other sites can follow you across them again. Private tabs keep it on",
        ),
        &toggle(b, !b.prefs.borrow().keeps_sign_ins, |b, on| {
            b.prefs.borrow_mut().keeps_sign_ins = !on;
            b.web.set_tracking_prevention(on);
        }),
    ));
    c.append(&rule(14));
    c.append(&line(
        "Let sites ask to send notifications",
        Some("A site asks at the bottom of its page. Private tabs are never asked"),
        &toggle(b, b.prefs.borrow().site_notifications, |b, on| b.prefs.borrow_mut().site_notifications = on),
    ));
    page.append(&c);

    let answers = b.prefs.borrow().permissions.clone();
    if !answers.is_empty() {
        let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
        section.append(&caption("Camera, microphone, location and notifications"));
        let c = card();
        for (i, (key, allowed)) in answers.iter().enumerate() {
            if i > 0 {
                c.append(&rule(14));
            }
            let forget = quick("Forget", false);
            let (weak, k) = (b.weak(), key.clone());
            forget.connect_clicked(move |_| {
                if let Some(b) = weak.upgrade() {
                    b.prefs.borrow_mut().permissions.remove(&k);
                    b.prefs.borrow().save();
                    b.ui().panels.refill();
                }
            });
            let (host, kind) = key.split_once(' ').unwrap_or((key, ""));
            c.append(&line(host, Some(&format!("{kind}: {}", if *allowed { "allowed" } else { "refused" })), &forget));
        }
        section.append(&c);
        page.append(&section);
    }

    let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
    section.append(&caption("What Wisp remembers"));
    section.append(&remembered(b, "Every address you have been to", || {}));
    page.append(&section);
}

fn about(page: &gtk::Box) {
    let c = card();
    c.append(&line(
        &format!("Wisp {}", env!("CARGO_PKG_VERSION")),
        Some("A small, fast, quiet browser. A Rust rewrite of Search by Office Commun, on the WebKitGTK already on your system."),
        &gtk::Box::new(gtk::Orientation::Horizontal, 0),
    ));
    c.append(&rule(14));
    c.append(&line(
        "No account, no sync, no telemetry",
        Some("History, bookmarks and open tabs are small files in ~/.local/share/wisp. Nothing is sent anywhere."),
        &gtk::Box::new(gtk::Orientation::Horizontal, 0),
    ));
    page.append(&c);
}

// MARK: pieces

/// History, cookies, and cache. The History panel and Settings share this card.
fn remembered(b: &Rc<Browser>, history_detail: &'static str, after_history: impl Fn() + 'static) -> gtk::Box {
    let c = card();
    let history = pill("Clear", false);
    let weak = b.weak();
    history.connect_clicked(move |_| {
        if let Some(b) = weak.upgrade() {
            b.history.borrow_mut().clear();
            b.history.borrow_mut().save();
            b.announce("History cleared");
        }
        after_history();
    });
    c.append(&line("History", Some(history_detail), &history));
    c.append(&rule(14));
    let cookies = pill("Sign out of everything", false);
    let weak = b.weak();
    cookies.connect_clicked(move |_| {
        if let Some(b) = weak.upgrade() {
            clear_site_data(&b, webkit6::WebsiteDataTypes::ALL, "Signed out of every site");
        }
    });
    c.append(&line("Cookies and sign-ins", Some("Signs you out of every site"), &cookies));
    c.append(&rule(14));
    let cache = pill("Clear", false);
    let weak = b.weak();
    cache.connect_clicked(move |_| {
        if let Some(b) = weak.upgrade() {
            clear_site_data(
                &b,
                webkit6::WebsiteDataTypes::DISK_CACHE | webkit6::WebsiteDataTypes::MEMORY_CACHE,
                "Cache cleared",
            );
        }
    });
    c.append(&line("Cache", Some("Only what was fetched to draw pages"), &cache));
    c
}

fn clear_site_data(b: &Rc<Browser>, kinds: webkit6::WebsiteDataTypes, said: &'static str) {
    let Some(manager) = b.web.session.website_data_manager() else { return };
    // WebKit answers on this thread; the weak reference only needs to be
    // sendable to satisfy the binding.
    let toasts: glib::SendWeakRef<adw::ApplicationWindow> = b.window.downgrade().into();
    manager.clear(kinds, glib::TimeSpan::from_seconds(0), None::<&gio::Cancellable>, move |result| {
        if let (Ok(()), Some(window)) = (result, toasts.upgrade()) {
            ActionGroupExt::activate_action(&window, "announce", Some(&said.to_variant()));
        }
    });
}

fn bring_in(b: &Rc<Browser>) {
    let report = b.bookmarks.borrow_mut().bring_in();
    if report.is_empty() {
        b.announce("No Chromium, Chrome, Brave, Vivaldi or Edge found");
    } else {
        let said: Vec<String> = report.iter().map(|(name, n)| format!("{n} from {name}")).collect();
        b.announce(&format!("Bookmarks brought in: {}", said.join(", ")));
    }
    b.ui().panels.refill();
}

fn ask_name(b: &Rc<Browser>, title: &str, done: impl Fn(&Rc<Browser>, &str) + 'static) {
    let dialog = adw::AlertDialog::new(Some(title), None);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Name"));
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancel"), ("create", "Create")]);
    dialog.set_default_response(Some("create"));
    let weak = b.weak();
    dialog.connect_response(Some("create"), move |_, _| {
        let name = entry.text().trim().to_string();
        if let (Some(b), false) = (weak.upgrade(), name.is_empty()) {
            done(&b, &name);
        }
    });
    dialog.present(Some(&b.window));
}

/// Folders and sites, folders opening in place.
fn outline(b: &Rc<Browser>, nodes: &[crate::bookmarks::Bookmark], into: &gtk::Box, depth: i32) {
    for node in nodes {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("outline-row");
        row.set_margin_start(depth * 16);
        let button = gtk::Button::new();
        button.add_css_class("outline");
        button.set_child(Some(&row));
        match &node.url {
            None => {
                let arrow = gtk::Image::from_icon_name("pan-end-symbolic");
                arrow.set_pixel_size(10);
                let folder = gtk::Image::from_icon_name("folder-symbolic");
                folder.set_pixel_size(13);
                folder.add_css_class("muted");
                row.append(&arrow);
                row.append(&folder);
                row.append(&gtk::Label::new(Some(&node.title)));
                let kids = gtk::Box::new(gtk::Orientation::Vertical, 0);
                kids.set_visible(false);
                outline(b, &node.children, &kids, depth + 1);
                let k = kids.clone();
                button.connect_clicked(move |_| {
                    k.set_visible(!k.is_visible());
                    arrow.set_icon_name(Some(if k.is_visible() { "pan-down-symbolic" } else { "pan-end-symbolic" }));
                });
                into.append(&button);
                into.append(&kids);
            }
            Some(url) => {
                row.append(&mark(url, 16));
                let title = gtk::Label::new(Some(&node.title));
                title.set_ellipsize(pango::EllipsizeMode::End);
                title.set_xalign(0.0);
                title.set_hexpand(true);
                row.append(&title);
                let (weak, u) = (b.weak(), url.clone());
                button.connect_clicked(move |_| {
                    if let Some(b) = weak.upgrade() {
                        b.ui().panels.close();
                        if let Some(p) = b.ui().panels.popover.borrow().clone() {
                            p.popdown();
                        }
                        b.go(&u);
                    }
                });
                let middle = gtk::GestureClick::new();
                middle.set_button(2);
                let (weak, u) = (b.weak(), url.clone());
                middle.connect_released(move |_, _, _, _| {
                    if let Some(b) = weak.upgrade() {
                        let from = b.active();
                        b.open(&u, false, from.as_ref());
                    }
                });
                button.add_controller(middle);
                into.append(&button);
            }
        }
    }
}

/// A letter in a faint square, standing for a site.
fn mark(url: &str, size: i32) -> gtk::Label {
    let letter = address::bare_host(url)
        .and_then(|h| h.chars().next())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "•".into());
    let m = gtk::Label::new(Some(&letter));
    m.add_css_class("mark");
    m.set_size_request(size, size);
    m
}

fn trace_row(
    url: &str,
    title: &str,
    sub: &str,
    trailing: &str,
    go: impl Fn() + 'static,
    forget: impl Fn() + 'static,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("list-row");
    row.append(&mark(url, 16));
    let text = two_lines(title, sub);
    text.set_hexpand(true);
    row.append(&text);
    let stack = gtk::Stack::new();
    let time = gtk::Label::new(Some(trailing));
    time.add_css_class("time");
    stack.add_named(&time, Some("time"));
    let remove = quick("Remove", true);
    remove.connect_clicked(move |_| forget());
    if !trailing.is_empty() {
        stack.add_named(&remove, Some("remove"));
    }
    row.append(&stack);
    let hover = gtk::EventControllerMotion::new();
    let (s, has) = (stack.clone(), !trailing.is_empty());
    hover.connect_enter(move |_, _, _| {
        if has {
            s.set_visible_child_name("remove");
        }
    });
    let s = stack.clone();
    hover.connect_leave(move |_| s.set_visible_child_name("time"));
    row.add_controller(hover);
    let click = gtk::GestureClick::new();
    let r = remove.clone();
    click.connect_released(move |g, _, x, y| {
        let hit = g.widget().and_then(|w| w.pick(x, y, gtk::PickFlags::DEFAULT));
        if !hit.is_some_and(|h| h == r.clone().upcast::<gtk::Widget>() || h.is_ancestor(&r)) {
            go();
        }
    });
    row.add_controller(click);
    row.upcast()
}

fn fetch_row(f: &crate::browser::Fetch) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("list-row");
    let icon = gtk::Image::from_icon_name(if f.failed.is_some() {
        "dialog-warning-symbolic"
    } else {
        "folder-download-symbolic"
    });
    icon.add_css_class("muted");
    row.append(&icon);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text.set_hexpand(true);
    let name = gtk::Label::new(Some(if f.name.is_empty() { "Download" } else { &f.name }));
    name.add_css_class("row-title");
    name.set_xalign(0.0);
    name.set_ellipsize(pango::EllipsizeMode::Middle);
    text.append(&name);
    let state = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    let fraction = f.download.estimated_progress();
    if f.failed.is_none() {
        let bar = gtk::ProgressBar::new();
        bar.add_css_class("thin");
        bar.set_fraction(fraction);
        bar.set_size_request(68, -1);
        bar.set_valign(gtk::Align::Center);
        state.append(&bar);
    }
    let received = f.download.received_data_length();
    let total = f.download.response().map(|r| r.content_length()).unwrap_or(0);
    let bytes = if total > 0 {
        format!("{} of {}", glib::format_size(received), glib::format_size(total))
    } else {
        format!("{} downloaded", glib::format_size(received))
    };
    let said = gtk::Label::new(Some(&match &f.failed {
        Some(why) => format!("Failed · {why}"),
        None => format!("Downloading · {bytes}"),
    }));
    said.add_css_class("row-sub");
    said.set_ellipsize(pango::EllipsizeMode::Middle);
    state.append(&said);
    text.append(&state);
    row.append(&text);
    let cancel = quick(if f.failed.is_some() { "Remove" } else { "Cancel" }, false);
    let d = f.download.clone();
    cancel.connect_clicked(move |_| d.cancel());
    row.append(&cancel);
    row.upcast()
}

fn kept_row(b: &Rc<Browser>, k: &crate::loot::Keep) -> gtk::Widget {
    let there = k.still_there();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("list-row");
    if !there {
        row.add_css_class("gone");
    }
    let icon = gtk::Image::from_icon_name("text-x-generic-symbolic");
    icon.add_css_class("muted");
    row.append(&icon);
    let when = relative(k.date);
    let text = two_lines(&k.name, &if k.from.is_empty() { when } else { format!("{} · {when}", k.from) });
    text.set_hexpand(true);
    row.append(&text);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    actions.set_opacity(0.0);
    if there {
        let show = quick("Show in Folder", false);
        let p = k.path.clone();
        show.connect_clicked(move |_| {
            let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&p)));
            launcher.open_containing_folder(None::<&gtk::Window>, None::<&gio::Cancellable>, |_| {});
        });
        actions.append(&show);
    }
    let remove = quick("Remove", true);
    let (weak, p) = (b.weak(), k.path.clone());
    remove.connect_clicked(move |_| {
        if let Some(b) = weak.upgrade() {
            b.loot.borrow_mut().forget(&p);
            b.ui().panels.refill();
        }
    });
    actions.append(&remove);
    row.append(&actions);
    let hover = gtk::EventControllerMotion::new();
    let a = actions.clone();
    hover.connect_enter(move |_, _, _| a.set_opacity(1.0));
    let a = actions.clone();
    hover.connect_leave(move |_| a.set_opacity(0.0));
    row.add_controller(hover);
    if there {
        let click = gtk::GestureClick::new();
        let (p, a) = (k.path.clone(), actions.clone());
        click.connect_released(move |g, _, x, y| {
            let hit = g.widget().and_then(|w| w.pick(x, y, gtk::PickFlags::DEFAULT));
            if hit.is_some_and(|h| h.is_ancestor(&a)) {
                return;
            }
            let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&p)));
            launcher.launch(None::<&gtk::Window>, None::<&gio::Cancellable>, |_| {});
        });
        row.add_controller(click);
    }
    row.upcast()
}

/// A panel's frame: title, a way out, what it holds, and a foot.
fn plate_of(
    title: &str,
    width: i32,
    content: &impl IsA<gtk::Widget>,
    foot: Option<&gtk::Widget>,
    close: impl Fn() + 'static,
) -> gtk::Widget {
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("plate");
    frame.set_size_request(width, -1);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    head.add_css_class("plate-head");
    let t = gtk::Label::new(Some(title));
    t.add_css_class("plate-title");
    t.set_xalign(0.0);
    t.set_hexpand(true);
    t.set_ellipsize(pango::EllipsizeMode::End);
    let done = crate::tabs::door("window-close-symbolic", "Done   esc");
    done.connect_clicked(move |_| close());
    head.append(&t);
    head.append(&done);
    frame.append(&head);
    content.add_css_class("plate-content");
    frame.append(content);
    match foot {
        Some(foot) => {
            frame.append(&rule(0));
            foot.add_css_class("plate-foot");
            frame.append(foot);
        }
        None => {
            let pad = gtk::Box::new(gtk::Orientation::Vertical, 0);
            pad.set_size_request(-1, 20);
            frame.append(&pad);
        }
    }
    frame.upcast()
}

fn plate(panels: &Rc<Panels>, title: &str, width: i32, content: &gtk::Box, foot: Option<&gtk::Box>) -> gtk::Widget {
    let me = Rc::downgrade(panels);
    plate_of(title, width, content, foot.map(|f| f.upcast_ref::<gtk::Widget>()), move || {
        if let Some(me) = me.upgrade() {
            me.close();
        }
    })
}

pub fn card() -> gtk::Box {
    let c = gtk::Box::new(gtk::Orientation::Vertical, 0);
    c.add_css_class("card");
    c.set_overflow(gtk::Overflow::Hidden);
    c
}

fn rule(inset: i32) -> gtk::Widget {
    let r = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    r.add_css_class("rule");
    r.set_margin_start(inset);
    r.upcast()
}

fn line(title: &str, detail: Option<&str>, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.add_css_class("line");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("line-title");
    t.set_xalign(0.0);
    t.set_wrap(true);
    text.append(&t);
    if let Some(d) = detail {
        let d = gtk::Label::new(Some(d));
        d.add_css_class("line-detail");
        d.set_xalign(0.0);
        d.set_wrap(true);
        d.set_lines(3);
        d.set_max_width_chars(48);
        text.append(&d);
    }
    row.append(&text);
    control.set_valign(gtk::Align::Center);
    if control.has_css_class("switch") {
        // A switch is known by its line's title, to a screen reader too.
        control
            .upcast_ref::<gtk::Widget>()
            .update_relation(&[gtk::accessible::Relation::LabelledBy(&[t.upcast_ref()])]);
    }
    row.append(control);
    row.upcast()
}

fn caption(text: &str) -> gtk::Label {
    let c = gtk::Label::new(Some(text));
    c.add_css_class("caption-text");
    c.set_xalign(0.0);
    c
}

fn nothing(text: &str) -> gtk::Widget {
    let c = card();
    let l = gtk::Label::new(Some(text));
    l.add_css_class("nothing");
    l.set_xalign(0.0);
    l.set_wrap(true);
    c.append(&l);
    c.upcast()
}

fn two_lines(title: &str, sub: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("row-title");
    t.set_xalign(0.0);
    t.set_ellipsize(pango::EllipsizeMode::End);
    b.append(&t);
    if !sub.is_empty() {
        let s = gtk::Label::new(Some(sub));
        s.add_css_class("row-sub");
        s.set_xalign(0.0);
        s.set_ellipsize(pango::EllipsizeMode::Middle);
        b.append(&s);
    }
    b
}

fn hunt(prompt: &str) -> (gtk::Box, gtk::Text) {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class("hunt");
    let glass = gtk::Image::from_icon_name("system-search-symbolic");
    glass.set_pixel_size(12);
    glass.add_css_class("muted");
    let t = crate::text_box(prompt);
    t.set_placeholder_text(Some(prompt));
    t.set_hexpand(true);
    b.append(&glass);
    b.append(&t);
    (b, t)
}

pub fn quick(title: &str, red: bool) -> gtk::Button {
    let q = gtk::Button::with_label(title);
    q.add_css_class("quick");
    if red {
        q.add_css_class("red");
    }
    q
}

pub fn pill(title: &str, filled: bool) -> gtk::Button {
    let p = gtk::Button::with_label(title);
    p.add_css_class("pill");
    if filled {
        p.add_css_class("filled");
    }
    p
}

fn foot_row(title: &str) -> gtk::Button {
    let b = gtk::Button::with_label(title);
    b.add_css_class("foot-row");
    if let Some(l) = b.child().and_downcast::<gtk::Label>() {
        l.set_xalign(0.0);
    }
    b
}

fn labelled(name: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let l = gtk::Label::new(Some(name));
    l.add_css_class("row-sub");
    l.set_size_request(42, -1);
    l.set_xalign(0.0);
    row.append(&l);
    control.set_hexpand(true);
    row.append(control);
    row.upcast()
}

/// On or off, in ink rather than in blue. The knob glides across.
fn switch(on: bool, changed: impl Fn(bool) + 'static) -> gtk::Widget {
    let knob = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    knob.add_css_class("knob");
    knob.set_halign(gtk::Align::Start);
    knob.set_valign(gtk::Align::Center);
    let knob = Slide::new(&knob);
    knob.set_halign(gtk::Align::Start);
    let button = gtk::Button::new();
    button.add_css_class("switch");
    button.set_child(Some(&knob));
    let pressed = |b: &gtk::Button, on: bool| {
        let state = if on { gtk::AccessibleTristate::True } else { gtk::AccessibleTristate::False };
        b.update_state(&[gtk::accessible::State::Pressed(state)]);
    };
    if on {
        button.add_css_class("on");
    }
    pressed(&button, on);
    let k = knob.clone();
    let travel = Tween::new(&knob, if on { 12.0 } else { 0.0 }, move |x| k.set_offset(x, 0.0));
    let state = Cell::new(on);
    button.connect_clicked(move |button| {
        let on = !state.get();
        state.set(on);
        if on {
            button.add_css_class("on")
        } else {
            button.remove_css_class("on")
        }
        pressed(button, on);
        travel.to(if on { 12.0 } else { 0.0 }, Curve::Settle);
        changed(on);
    });
    button.upcast()
}

/// A choice of a few, the chosen one lifted out on a ground that slides.
fn segmented<T: Copy + PartialEq + 'static>(
    options: &[(T, &str)],
    selected: T,
    changed: impl Fn(T) + 'static,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let chosen = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    chosen.add_css_class("chosen");
    let layer = gtk::Fixed::new();
    layer.put(&chosen, 0.0, 0.0);
    let frame = gtk::Overlay::new();
    frame.add_css_class("segmented");
    frame.set_child(Some(&layer));
    frame.add_overlay(&row);
    frame.set_measure_overlay(&row, true);
    let (l, c) = (layer.clone(), chosen.clone());
    let x = Tween::new(&layer, 0.0, move |v| l.move_(&c, v, 0.0));
    let buttons: Rc<RefCell<Vec<(T, gtk::Button)>>> = Rc::default();
    let buttons_weak = Rc::downgrade(&buttons);
    let row_weak = row.downgrade();
    let chosen_weak = chosen.downgrade();
    let place: Rc<dyn Fn(T, bool)> = {
        let x = x.clone();
        Rc::new(move |value, animated| {
            let (Some(buttons), Some(row), Some(chosen)) =
                (buttons_weak.upgrade(), row_weak.upgrade(), chosen_weak.upgrade())
            else {
                return;
            };
            for (v, b) in buttons.borrow().iter() {
                if *v == value {
                    b.add_css_class("on");
                    if let Some(bounds) = b.compute_bounds(&row) {
                        chosen.set_size_request(bounds.width() as i32, bounds.height() as i32);
                        if animated { x.to(bounds.x() as f64, Curve::Settle) } else { x.set(bounds.x() as f64) }
                    }
                } else {
                    b.remove_css_class("on");
                }
            }
        })
    };
    let changed = Rc::new(changed);
    for (value, title) in options {
        let b = gtk::Button::with_label(title);
        b.add_css_class("segment");
        let (place, changed, value) = (place.clone(), changed.clone(), *value);
        b.connect_clicked(move |_| {
            place(value, true);
            changed(value);
        });
        row.append(&b);
        buttons.borrow_mut().push((value, b));
    }
    let keeper = buttons.clone();
    let p = place.clone();
    row.connect_map(move |_| {
        let _ = &keeper;
        let p = p.clone();
        glib::idle_add_local_once(move || p(selected, false));
    });
    frame.upcast()
}

/// A value moved one stop at a time, − and + either side of it.
fn steps(stops: &'static [f64], value: f64, home: f64, changed: impl Fn(f64) + 'static) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("steps");
    let minus = gtk::Button::from_icon_name("list-remove-symbolic");
    let plus = gtk::Button::from_icon_name("list-add-symbolic");
    let shown = gtk::Button::with_label("");
    for b in [&minus, &plus] {
        b.add_css_class("step");
    }
    shown.add_css_class("step-value");
    row.append(&minus);
    row.append(&shown);
    row.append(&plus);
    let value = Rc::new(Cell::new(value));
    let changed = Rc::new(changed);
    let show: Rc<dyn Fn()> = {
        let value = value.clone();
        let shown = shown.downgrade();
        let minus = minus.downgrade();
        let plus = plus.downgrade();
        Rc::new(move || {
            let (Some(shown), Some(minus), Some(plus)) = (shown.upgrade(), minus.upgrade(), plus.upgrade()) else {
                return;
            };
            let v = value.get();
            shown.set_label(&format!("{}%", (v * 100.0).round()));
            minus.set_sensitive(stops.iter().any(|&s| s < v - 0.001));
            plus.set_sensitive(stops.iter().any(|&s| s > v + 0.001));
        })
    };
    show();
    let set = {
        let (value, changed, show) = (value.clone(), changed.clone(), show.clone());
        Rc::new(move |v: f64| {
            value.set(v);
            changed(v);
            show();
        })
    };
    let (s, v) = (set.clone(), value.clone());
    minus.connect_clicked(move |_| {
        if let Some(&below) = stops.iter().rev().find(|&&x| x < v.get() - 0.001) {
            s(below);
        }
    });
    let (s, v) = (set.clone(), value.clone());
    plus.connect_clicked(move |_| {
        if let Some(&above) = stops.iter().find(|&&x| x > v.get() + 0.001) {
            s(above);
        }
    });
    shown.connect_clicked(move |_| set(home));
    row.upcast()
}

fn scroll(child: &impl IsA<gtk::Widget>, most: i32) -> gtk::ScrolledWindow {
    let s = gtk::ScrolledWindow::new();
    s.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    s.set_propagate_natural_height(true);
    s.set_max_content_height(most);
    s.set_child(Some(child));
    s
}

fn spacer() -> gtk::Box {
    let s = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    s.set_hexpand(true);
    s
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn focus_later(t: &gtk::Text) {
    let t = t.clone();
    glib::idle_add_local_once(move || {
        t.grab_focus();
    });
}

fn local(ts: i64) -> Option<glib::DateTime> {
    glib::DateTime::from_unix_local(ts).ok()
}

/// "Today", "Yesterday", or "3 March".
fn day_of(ts: i64) -> String {
    let (Some(d), Ok(now)) = (local(ts), glib::DateTime::now_local()) else { return String::new() };
    let days = |x: &glib::DateTime| (x.year(), x.day_of_year());
    if days(&d) == days(&now) {
        return "Today".into();
    }
    if let Ok(y) = now.add_days(-1)
        && days(&d) == days(&y)
    {
        return "Yesterday".into();
    }
    d.format("%-d %B").map(|s| s.to_string()).unwrap_or_default()
}

fn clock(ts: i64) -> String {
    local(ts).and_then(|d| d.format("%H:%M").ok()).map(|s| s.to_string()).unwrap_or_default()
}

/// "just now", "5 min. ago", "2 hr. ago", "3 days ago".
fn relative(ts: i64) -> String {
    let ago = (crate::history::now() - ts).max(0);
    match ago {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min. ago", ago / 60),
        3600..=86_399 => format!("{} hr. ago", ago / 3600),
        _ => format!("{} days ago", ago / 86_400),
    }
}
