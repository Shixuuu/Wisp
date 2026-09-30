//! The panels: tab switcher, history, bookmarks, downloads, what's hidden on
//! this site, and the list of shortcuts. Each is a searchable list in a
//! dialog, one keystroke away, gone with Esc.

use crate::address;
use crate::browser::Browser;
use crate::curtain;
use crate::downloads;
use adw::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;
use webkit6::prelude::*;

/// One row in a panel.
pub struct Row {
    pub title: String,
    pub subtitle: String,
    pub icon: Option<gtk::gdk::Paintable>,
    pub open: Rc<dyn Fn()>,
    /// A trailing button: (icon, tooltip, what it does).
    pub extra: Option<(&'static str, &'static str, Act)>,
}

type Fill = Rc<dyn Fn(&str) -> Vec<Row>>;
type Act = Rc<dyn Fn()>;

impl Browser {
    /// A searchable list in a dialog. `fill` is asked for rows each time the
    /// search changes (and after a row's extra button is used).
    fn panel(self: &Rc<Self>, title: &str, placeholder: &str, empty: &str, fill: Fill, footer: Option<gtk::Widget>) {
        let dialog = adw::Dialog::builder().title(title).content_width(560).content_height(560).build();
        let search = gtk::SearchEntry::builder().placeholder_text(placeholder).hexpand(true).build();
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_valign(gtk::Align::Start);
        let status = adw::StatusPage::builder().title(empty).icon_name("edit-find-symbolic").build();
        status.add_css_class("compact");
        let stack = gtk::Stack::new();
        let scroll =
            gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&list).build();
        stack.add_named(&scroll, Some("list"));
        stack.add_named(&status, Some("empty"));

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&search);
        content.append(&stack);
        if let Some(footer) = footer {
            content.append(&footer);
        }
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&content));
        dialog.set_child(Some(&toolbar));

        let actions: Rc<RefCell<Vec<Act>>> = Rc::new(RefCell::new(Vec::new()));
        let refill: Rc<RefCell<Option<Act>>> = Rc::new(RefCell::new(None));
        let run = {
            let list = list.clone();
            let stack = stack.clone();
            let search = search.clone();
            let actions = actions.clone();
            let refill_ref = refill.clone();
            let dialog = dialog.clone();
            Rc::new(move || {
                while let Some(child) = list.first_child() {
                    list.remove(&child);
                }
                let rows = fill(&search.text());
                let mut opens = Vec::new();
                for row in rows.into_iter().take(400) {
                    let item = adw::ActionRow::builder()
                        .title(glib::markup_escape_text(&row.title))
                        .subtitle(glib::markup_escape_text(&row.subtitle))
                        .activatable(true)
                        .title_lines(1)
                        .subtitle_lines(1)
                        .build();
                    if let Some(icon) = &row.icon {
                        let image = gtk::Image::from_paintable(Some(icon));
                        image.set_pixel_size(16);
                        item.add_prefix(&image);
                    }
                    if let Some((icon, tip, act)) = row.extra.clone() {
                        let button = gtk::Button::from_icon_name(icon);
                        button.set_tooltip_text(Some(tip));
                        button.add_css_class("flat");
                        button.set_valign(gtk::Align::Center);
                        let refill = refill_ref.clone();
                        button.connect_clicked(move |_| {
                            act();
                            if let Some(r) = refill.borrow().as_ref() {
                                let r = r.clone();
                                glib::idle_add_local_once(move || r());
                            }
                        });
                        item.add_suffix(&button);
                    }
                    list.append(&item);
                    let open = row.open.clone();
                    let dialog = dialog.clone();
                    opens.push(Rc::new(move || {
                        dialog.close();
                        open();
                    }) as Rc<dyn Fn()>);
                }
                stack.set_visible_child_name(if opens.is_empty() { "empty" } else { "list" });
                if let Some(first) = list.row_at_index(0) {
                    list.select_row(Some(&first));
                }
                *actions.borrow_mut() = opens;
            })
        };
        *refill.borrow_mut() = Some(run.clone());
        run();

        let r = run.clone();
        search.connect_search_changed(move |_| r());
        // Esc in the search closes the panel (the entry would otherwise
        // just clear itself).
        let d = dialog.clone();
        search.connect_stop_search(move |_| {
            d.close();
        });
        let a = actions.clone();
        list.connect_row_activated(move |_, row| {
            let open = a.borrow().get(row.index() as usize).cloned();
            if let Some(open) = open {
                open();
            }
        });
        let a = actions.clone();
        let l = list.clone();
        search.connect_activate(move |_| {
            let at = l.selected_row().map(|r| r.index()).unwrap_or(0) as usize;
            let open = a.borrow().get(at).cloned();
            if let Some(open) = open {
                open();
            }
        });
        // Up and Down move through the list without leaving the search.
        let keys = gtk::EventControllerKey::new();
        let l = list.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            use gtk::gdk::Key;
            if key != Key::Down && key != Key::Up {
                return glib::Propagation::Proceed;
            }
            let at = l.selected_row().map(|r| r.index()).unwrap_or(-1);
            let next = if key == Key::Down { at + 1 } else { (at - 1).max(0) };
            if let Some(row) = l.row_at_index(next) {
                l.select_row(Some(&row));
            }
            glib::Propagation::Stop
        });
        search.add_controller(keys);

        let weak = self.weak();
        dialog.connect_closed(move |_| {
            if let Some(b) = weak.upgrade()
                && let Some(v) = b.current_view()
            {
                v.grab_focus();
            }
        });
        dialog.present(Some(&self.window));
        search.grab_focus();
    }

    pub fn show_tab_switcher(self: &Rc<Self>) {
        let weak = self.weak();
        let fill: Fill = Rc::new(move |q| {
            let Some(b) = weak.upgrade() else { return Vec::new() };
            let q = q.to_lowercase();
            b.tabs
                .borrow()
                .iter()
                .filter(|t| {
                    q.is_empty() || t.name().to_lowercase().contains(&q) || t.url.borrow().to_lowercase().contains(&q)
                })
                .map(|t| {
                    let weak = b.weak();
                    let id = t.id;
                    let weak2 = b.weak();
                    Row {
                        title: t.name(),
                        subtitle: address::pretty(&t.url.borrow()),
                        icon: t.row.icon.paintable(),
                        open: Rc::new(move || {
                            if let Some(b) = weak.upgrade()
                                && let Some(t) = b.tab_by_id(id)
                            {
                                b.select(&t);
                            }
                        }),
                        extra: Some((
                            "window-close-symbolic",
                            "Close tab",
                            Rc::new(move || {
                                if let Some(b) = weak2.upgrade() {
                                    b.close_tab(id);
                                }
                            }),
                        )),
                    }
                })
                .collect()
        });
        self.panel("Tabs", "Find an open tab", "No tab by that name", fill, None);
    }

    pub fn show_history(self: &Rc<Self>) {
        let weak = self.weak();
        let fill: Fill = Rc::new(move |q| {
            let Some(b) = weak.upgrade() else { return Vec::new() };
            let visits = b.history.borrow().everything(q);
            visits
                .into_iter()
                .take(400)
                .map(|v| {
                    let weak = b.weak();
                    let weak2 = b.weak();
                    let url = v.url.clone();
                    let url2 = v.url.clone();
                    Row {
                        title: if v.title.is_empty() { address::pretty(&v.url) } else { v.title.clone() },
                        subtitle: format!("{} · {}", address::pretty(&v.url), when(v.last)),
                        icon: None,
                        open: Rc::new(move || {
                            if let Some(b) = weak.upgrade() {
                                b.open(&url, false);
                            }
                        }),
                        extra: Some((
                            "user-trash-symbolic",
                            "Forget this page",
                            Rc::new(move || {
                                if let Some(b) = weak2.upgrade() {
                                    b.history.borrow_mut().forget(&url2);
                                }
                            }),
                        )),
                    }
                })
                .collect()
        });
        let clear = gtk::Button::with_label("Clear All History…");
        clear.add_css_class("destructive-action");
        clear.set_halign(gtk::Align::End);
        let weak = self.weak();
        clear.connect_clicked(move |button| {
            let Some(b) = weak.upgrade() else { return };
            let dialog = adw::AlertDialog::new(
                Some("Clear all history?"),
                Some("Every page you have visited is forgotten. Bookmarks and open tabs stay."),
            );
            dialog.add_responses(&[("cancel", "Cancel"), ("clear", "Clear")]);
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let weak = b.weak();
            dialog.connect_response(Some("clear"), move |_, _| {
                if let Some(b) = weak.upgrade() {
                    b.history.borrow_mut().clear();
                    b.history.borrow_mut().save();
                    b.toast("History cleared");
                }
            });
            dialog.present(Some(button));
        });
        self.panel("History", "Search history", "Nothing here yet", fill, Some(clear.upcast()));
    }

    pub fn show_bookmarks(self: &Rc<Self>) {
        let weak = self.weak();
        let fill: Fill = Rc::new(move |q| {
            let Some(b) = weak.upgrade() else { return Vec::new() };
            let items = b.bookmarks.borrow().matching(q);
            items
                .into_iter()
                .rev()
                .map(|bm| {
                    let weak = b.weak();
                    let weak2 = b.weak();
                    let url = bm.url.clone();
                    let url2 = bm.url.clone();
                    let sub = if bm.folder.is_empty() {
                        address::pretty(&bm.url)
                    } else {
                        format!("{} · {}", bm.folder, address::pretty(&bm.url))
                    };
                    Row {
                        title: bm.title.clone(),
                        subtitle: sub,
                        icon: None,
                        open: Rc::new(move || {
                            if let Some(b) = weak.upgrade() {
                                b.open(&url, false);
                            }
                        }),
                        extra: Some((
                            "user-trash-symbolic",
                            "Remove bookmark",
                            Rc::new(move || {
                                if let Some(b) = weak2.upgrade() {
                                    b.bookmarks.borrow_mut().remove(&url2);
                                }
                            }),
                        )),
                    }
                })
                .collect()
        });
        let import = gtk::Button::with_label("Import from Chromium, Chrome, Brave…");
        import.set_halign(gtk::Align::End);
        let weak = self.weak();
        import.connect_clicked(move |_| {
            let Some(b) = weak.upgrade() else { return };
            let report = b.bookmarks.borrow_mut().import_chromium();
            if report.is_empty() {
                b.toast("No Chromium-family browser found in ~/.config");
            } else {
                let text: Vec<String> = report.iter().map(|(n, c)| format!("{c} from {n}")).collect();
                b.toast(&format!("Imported {}", text.join(", ")));
            }
        });
        self.panel(
            "Bookmarks",
            "Search bookmarks",
            "No bookmarks yet. Ctrl+D bookmarks a page.",
            fill,
            Some(import.upcast()),
        );
    }

    pub fn show_downloads(self: &Rc<Self>) {
        let weak = self.weak();
        let fill: Fill = Rc::new(move |q| {
            let Some(b) = weak.upgrade() else { return Vec::new() };
            let q = q.to_lowercase();
            let downloads = b.downloads.borrow();
            downloads
                .iter()
                .rev()
                .filter(|i| i.name.to_lowercase().contains(&q))
                .map(|i| {
                    let state = if i.failed {
                        "Failed or cancelled".to_string()
                    } else if i.done {
                        "Done".to_string()
                    } else {
                        format!("{}%", (i.download.estimated_progress() * 100.0).round())
                    };
                    let path = i.path.clone();
                    let path2 = i.path.clone();
                    let dl = i.download.clone();
                    let running = !i.done && !i.failed;
                    Row {
                        title: if i.name.is_empty() { "Download".into() } else { i.name.clone() },
                        subtitle: state,
                        icon: None,
                        open: Rc::new(move || {
                            if let Some(p) = &path {
                                downloads::open_file(p);
                            }
                        }),
                        extra: Some(if running {
                            ("process-stop-symbolic", "Cancel", Rc::new(move || dl.cancel()) as Rc<dyn Fn()>)
                        } else {
                            (
                                "folder-open-symbolic",
                                "Show in folder",
                                Rc::new(move || {
                                    if let Some(p) = &path2 {
                                        downloads::show_in_folder(p);
                                    }
                                }) as Rc<dyn Fn()>,
                            )
                        }),
                    }
                })
                .collect()
        });
        self.panel("Downloads", "Search downloads", "Nothing downloaded this session", fill, None);
    }

    /// What is hidden on this site, and a way to bring each thing back.
    pub fn show_hidden(self: &Rc<Self>) {
        let Some(tab) = self.current.borrow().clone() else { return };
        let Some(host) = curtain::host_key(&tab.url.borrow()) else { return };
        let weak = self.weak();
        let h = host.clone();
        let fill: Fill = Rc::new(move |q| {
            let Some(b) = weak.upgrade() else { return Vec::new() };
            let q = q.to_lowercase();
            let veils = b.curtain.borrow().veils(&h).to_vec();
            veils
                .into_iter()
                .filter(|v| v.label.to_lowercase().contains(&q) || v.selector.to_lowercase().contains(&q))
                .map(|v| {
                    let weak = b.weak();
                    let host = h.clone();
                    let sel = v.selector.clone();
                    let restore: Rc<dyn Fn()> = Rc::new(move || {
                        if let Some(b) = weak.upgrade() {
                            b.curtain.borrow_mut().restore(&host, &sel);
                            b.retune_all();
                            if let Some(v) = b.current_view() {
                                v.reload();
                            }
                        }
                    });
                    Row {
                        title: v.label.clone(),
                        subtitle: if v.note.is_empty() {
                            v.selector.clone()
                        } else {
                            format!("{} · {}", v.note, v.selector)
                        },
                        icon: None,
                        open: restore.clone(),
                        extra: Some(("edit-undo-symbolic", "Bring it back", restore)),
                    }
                })
                .collect()
        });
        let all = gtk::Button::with_label("Bring Everything Back");
        all.set_halign(gtk::Align::End);
        let weak = self.weak();
        let h = host.clone();
        all.connect_clicked(move |_| {
            if let Some(b) = weak.upgrade() {
                b.curtain.borrow_mut().restore_all(&h);
                b.retune_all();
                if let Some(v) = b.current_view() {
                    v.reload();
                }
                b.toast("Everything is back");
            }
        });
        self.panel(
            &format!("Hidden on {host}"),
            "Search what's hidden",
            "Nothing hidden here. Ctrl+Shift+H hides something.",
            fill,
            Some(all.upcast()),
        );
    }

    pub fn show_shortcuts(self: &Rc<Self>) {
        let rows: &[(&str, &str)] = &[
            ("Ctrl+L", "Address field"),
            ("Ctrl+K", "Switch to an open tab"),
            ("Ctrl+T / Ctrl+W", "New tab / close tab"),
            ("Ctrl+Shift+T", "Reopen closed tab"),
            ("Ctrl+Shift+N", "New private tab"),
            ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous tab"),
            ("Ctrl+1 … Ctrl+9", "Jump to tab (9 is the last)"),
            ("Ctrl+Shift+D", "Duplicate tab"),
            ("Ctrl+Alt+P", "Pin or unpin tab"),
            ("Alt+← / Alt+→", "Back / forward"),
            ("Ctrl+R / Ctrl+Shift+R", "Reload / reload from the server"),
            ("Ctrl+F, Ctrl+G", "Find, find next"),
            ("Ctrl+Alt+R or F9", "Reading mode"),
            ("Ctrl+Shift+H", "Hide something on the page"),
            ("Ctrl+Shift+U", "What's hidden on this site"),
            ("Ctrl+D", "Bookmark this page"),
            ("Ctrl+Shift+O", "Bookmarks"),
            ("Ctrl+H", "History"),
            ("Ctrl+J", "Downloads"),
            ("Ctrl+Shift+S", "Tabs on the left or on top"),
            ("Ctrl+B", "Fold the tabs away"),
            ("Ctrl+Shift+C", "Copy address"),
            ("Ctrl+Shift+V", "Paste and go"),
            ("Ctrl+ + / − / 0", "Zoom"),
            ("F11", "Full screen"),
            ("F12", "Web inspector"),
            ("Ctrl+,", "Settings"),
        ];
        let fill: Fill = Rc::new(move |q| {
            let q = q.to_lowercase();
            rows.iter()
                .filter(|(k, d)| k.to_lowercase().contains(&q) || d.to_lowercase().contains(&q))
                .map(|(k, d)| Row {
                    title: d.to_string(),
                    subtitle: k.to_string(),
                    icon: None,
                    open: Rc::new(|| {}),
                    extra: None,
                })
                .collect()
        });
        self.panel("Keyboard Shortcuts", "Search shortcuts", "No shortcut by that name", fill, None);
    }
}

/// "3 min ago", "yesterday", "12 Mar".
fn when(ts: i64) -> String {
    let now = crate::history::now();
    let ago = (now - ts).max(0);
    match ago {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", ago / 60),
        3600..=86_399 => format!("{} h ago", ago / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => glib::DateTime::from_unix_local(ts)
            .ok()
            .and_then(|d| d.format("%-d %b %Y").ok())
            .map(|s| s.to_string())
            .unwrap_or_default(),
    }
}
