//! Every command, as a window action with its keyboard shortcut.
//!
//! Shortcuts follow Search's where they don't clash with what Linux browsers
//! have taught everyone (Ctrl+D bookmarks, Ctrl+Shift+R reloads hard, Ctrl+H
//! shows history). The README lists them all.

use crate::browser::Browser;
use crate::curtain;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::rc::Rc;
use webkit6::prelude::*;

impl Browser {
    fn action(self: &Rc<Self>, name: &str, accels: &[&str], run: impl Fn(&Rc<Browser>) + 'static) {
        let action = gio::SimpleAction::new(name, None);
        let weak = self.weak();
        action.connect_activate(move |_, _| {
            if let Some(b) = weak.upgrade() {
                run(&b);
            }
        });
        self.window.add_action(&action);
        if !accels.is_empty() {
            self.app.set_accels_for_action(&format!("win.{name}"), accels);
        }
    }

    /// An action that acts on one tab, named by its id (from a tab's menu).
    fn tab_action(self: &Rc<Self>, name: &str, run: impl Fn(&Rc<Browser>, u64) + 'static) {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::UINT64));
        let weak = self.weak();
        action.connect_activate(move |_, param| {
            if let (Some(b), Some(id)) = (weak.upgrade(), param.and_then(|p| p.get::<u64>())) {
                run(&b, id);
            }
        });
        self.window.add_action(&action);
    }

    fn with_view(&self, f: impl FnOnce(&webkit6::WebView)) {
        if let Some(v) = self.current_view() {
            f(&v);
        }
    }

    fn current_id(&self) -> Option<u64> {
        self.current.borrow().as_ref().map(|t| t.id)
    }

    pub fn install_actions(self: &Rc<Self>) {
        // Tabs.
        self.action("new-tab", &["<Ctrl>t"], |b| b.new_tab(false));
        self.action("new-private-tab", &["<Ctrl><Shift>n", "<Ctrl><Shift>p"], |b| b.new_tab(true));
        self.action("close-tab", &["<Ctrl>w", "<Ctrl>F4"], |b| {
            if let Some(id) = b.current_id() {
                b.close_tab(id);
            }
        });
        self.action("reopen-tab", &["<Ctrl><Shift>t"], |b| b.reopen());
        self.action("duplicate-tab", &["<Ctrl><Shift>d"], |b| {
            if let Some(id) = b.current_id() {
                b.duplicate(id);
            }
        });
        self.action("pin-tab", &["<Ctrl><Alt>p"], |b| {
            if let Some(id) = b.current_id() {
                b.toggle_pin(id);
            }
        });
        self.action("next-tab", &["<Ctrl>Tab", "<Ctrl>Page_Down"], |b| b.step(1));
        self.action("previous-tab", &["<Ctrl><Shift>Tab", "<Ctrl>Page_Up", "<Ctrl><Shift>ISO_Left_Tab"], |b| {
            b.step(-1)
        });
        for n in 1..=9usize {
            let accel = format!("<Ctrl>{n}");
            let alt = format!("<Alt>{n}");
            self.action(&format!("tab-{n}"), &[&accel, &alt], move |b| b.jump(n));
        }
        self.action("switch-tab", &["<Ctrl>k"], |b| b.show_tab_switcher());

        self.tab_action("tab-pin", |b, id| b.toggle_pin(id));
        self.tab_action("tab-duplicate", |b, id| b.duplicate(id));
        self.tab_action("tab-close", |b, id| b.close_tab(id));
        self.tab_action("tab-reload", |b, id| {
            if let Some(v) = b.tab_by_id(id).and_then(|t| t.view.borrow().clone()) {
                v.reload();
            }
        });
        self.tab_action("tab-mute", |b, id| {
            if let Some(v) = b.tab_by_id(id).and_then(|t| t.view.borrow().clone()) {
                v.set_is_muted(!v.is_muted());
            }
        });
        self.tab_action("tab-close-others", |b, id| {
            let others: Vec<u64> =
                b.tabs.borrow().iter().filter(|t| t.id != id && !t.pinned.get()).map(|t| t.id).collect();
            for other in others {
                b.close_tab(other);
            }
        });

        // The field.
        self.action("focus-address", &["<Ctrl>l", "<Alt>d", "F6"], |b| b.focus_field());
        self.action("copy-address", &["<Ctrl><Shift>c"], |b| {
            let url = b.current.borrow().as_ref().map(|t| t.url.borrow().clone()).unwrap_or_default();
            if !url.is_empty() {
                b.window.clipboard().set_text(&url);
                b.toast("Address copied");
            }
        });
        self.action("paste-and-go", &["<Ctrl><Shift>v"], |b| {
            let weak = b.weak();
            b.window.clipboard().read_text_async(None::<&gio::Cancellable>, move |text| {
                if let (Some(b), Ok(Some(text))) = (weak.upgrade(), text) {
                    b.navigate(&text);
                }
            });
        });

        // The page.
        self.action("back", &["<Alt>Left", "<Ctrl>bracketleft", "Back"], |b| b.with_view(|v| v.go_back()));
        self.action("forward", &["<Alt>Right", "<Ctrl>bracketright", "Forward"], |b| b.with_view(|v| v.go_forward()));
        self.action("reload", &["<Ctrl>r", "F5", "Reload"], |b| b.with_view(|v| v.reload()));
        self.action("reload-hard", &["<Ctrl><Shift>r", "<Shift>F5"], |b| b.with_view(|v| v.reload_bypass_cache()));
        self.action("stop", &[], |b| {
            if b.picking.get() {
                b.stop_picking();
            } else if b.find_bar.reveals_child() {
                b.close_find();
            } else {
                b.with_view(|v| v.stop_loading());
            }
        });
        self.action("zoom-in", &["<Ctrl>plus", "<Ctrl>equal", "<Ctrl>KP_Add"], |b| b.zoom(Some(0.1)));
        self.action("zoom-out", &["<Ctrl>minus", "<Ctrl>KP_Subtract"], |b| b.zoom(Some(-0.1)));
        self.action("zoom-reset", &["<Ctrl>0", "<Ctrl>KP_0"], |b| b.zoom(None));
        self.action("reader", &["<Ctrl><Alt>r", "F9"], |b| b.read());
        self.action("print", &["<Ctrl>p"], |b| {
            b.with_view(|v| {
                let op = webkit6::PrintOperation::new(v);
                op.run_dialog(Some(&b.window));
            })
        });
        self.action("inspector", &["F12", "<Ctrl><Shift>i"], |b| {
            b.with_view(|v| {
                if let Some(i) = v.inspector() {
                    i.show();
                }
            })
        });
        self.action("view-source", &["<Ctrl>u"], |b| {
            let url = b.current.borrow().as_ref().map(|t| t.url.borrow().clone()).unwrap_or_default();
            if url.starts_with("http") {
                b.open(&format!("view-source:{url}"), false);
            }
        });

        // Find.
        self.action("find", &["<Ctrl>f"], |b| b.open_find());
        self.action("find-next", &["<Ctrl>g", "F3"], |b| b.find_step(true));
        self.action("find-previous", &["<Ctrl><Shift>g", "<Shift>F3"], |b| b.find_step(false));
        self.action("find-close", &[], |b| b.close_find());

        // Hiding things.
        self.action("hide-element", &["<Ctrl><Shift>h"], |b| {
            if b.picking.get() {
                b.stop_picking();
            } else {
                b.start_picking();
            }
        });
        self.action("hidden-here", &["<Ctrl><Shift>u"], |b| b.show_hidden());
        self.action("shield-site", &[], |b| b.toggle_shield_for_site());

        // Layout.
        self.action("toggle-layout", &["<Ctrl><Shift>s"], |b| {
            let left = !b.prefs.borrow().tabs_on_left;
            b.prefs.borrow_mut().tabs_on_left = left;
            b.prefs.borrow().save();
            b.arrange();
        });
        self.action("toggle-sidebar", &["<Ctrl>b"], |b| {
            let folded = !b.prefs.borrow().sidebar_folded;
            b.prefs.borrow_mut().sidebar_folded = folded;
            b.prefs.borrow().save();
            b.set_chrome_visible(!folded);
            if folded {
                b.toast("Tabs folded away. Ctrl+B brings them back.");
            }
        });
        self.action("fullscreen", &["F11"], |b| {
            let on = !b.window.is_fullscreen();
            b.fullscreen.set(on);
            b.set_chrome_visible(!on && !b.prefs.borrow().sidebar_folded);
            if on {
                b.window.fullscreen();
            } else {
                b.window.unfullscreen();
            }
        });

        // Panels.
        self.action("bookmark-page", &["<Ctrl>d"], |b| b.toggle_bookmark());
        self.action("bookmarks", &["<Ctrl><Shift>o"], |b| b.show_bookmarks());
        self.action("history", &["<Ctrl>h", "<Ctrl>y"], |b| b.show_history());
        self.action("downloads", &["<Ctrl>j", "<Ctrl><Shift>j"], |b| b.show_downloads());
        self.action("settings", &["<Ctrl>comma"], |b| b.show_settings());
        self.action("shortcuts", &["<Ctrl>question", "<Ctrl>slash"], |b| b.show_shortcuts());
        self.action("quit", &["<Ctrl>q"], |b| b.window.close());

        // Esc: stop picking, close find, stop loading. Captured before the
        // page so it works even while the page has the keyboard.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.weak();
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(b) = weak.upgrade() else { return glib::Propagation::Proceed };
            if key == gdk::Key::Escape && (b.picking.get() || b.find_bar.reveals_child()) {
                if b.picking.get() {
                    b.stop_picking();
                } else {
                    b.close_find();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);

        // Mouse back/forward buttons.
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.weak();
        click.connect_pressed(move |g, _, _, _| {
            let Some(b) = weak.upgrade() else { return };
            match g.current_button() {
                8 => b.with_view(|v| v.go_back()),
                9 => b.with_view(|v| v.go_forward()),
                _ => {}
            }
        });
        self.window.add_controller(click);
    }

    fn zoom(&self, by: Option<f64>) {
        let Some(tab) = self.current.borrow().clone() else { return };
        let default = self.prefs.borrow().default_zoom;
        let level = match by {
            Some(d) => (tab.zoom.get() + d).clamp(0.3, 3.0),
            None => default,
        };
        tab.zoom.set(level);
        if let Some(v) = tab.view.borrow().as_ref() {
            v.set_zoom_level(level);
        }
        self.toast(&format!("Zoom {}%", (level * 100.0).round()));
    }

    fn toggle_bookmark(&self) {
        let Some(tab) = self.current.borrow().clone() else { return };
        let url = tab.url.borrow().clone();
        if !url.starts_with("http") {
            return;
        }
        let added = self.bookmarks.borrow_mut().toggle(&url, &tab.name());
        self.toast(if added { "Bookmarked" } else { "Bookmark removed" });
    }

    fn toggle_shield_for_site(self: &Rc<Self>) {
        let Some(tab) = self.current.borrow().clone() else { return };
        let Some(host) = curtain::host_key(&tab.url.borrow()) else { return };
        let paused = !self.prefs.borrow().is_paused(&host);
        self.prefs.borrow_mut().set_paused(&host, paused);
        self.prefs.borrow().save();
        let url = tab.url.borrow().clone();
        self.tune(&tab, &url);
        if let Some(v) = tab.view.borrow().as_ref() {
            v.reload();
        }
        self.toast(&if paused { format!("Blocker off for {host}") } else { format!("Blocker on for {host}") });
    }

    // MARK: find

    pub fn wire_find(self: &Rc<Self>) {
        let weak = self.weak();
        self.find_entry.connect_search_changed(move |entry| {
            let Some(b) = weak.upgrade() else { return };
            let text = entry.text().to_string();
            b.with_view(|v| {
                let Some(finder) = v.find_controller() else { return };
                if text.is_empty() {
                    finder.search_finish();
                    b.find_count.set_label("");
                    return;
                }
                let options = (webkit6::FindOptions::CASE_INSENSITIVE | webkit6::FindOptions::WRAP_AROUND).bits();
                finder.count_matches(&text, options, 1000);
                finder.search(&text, options, 1000);
            });
        });
        let weak = self.weak();
        self.find_entry.connect_activate(move |_| {
            if let Some(b) = weak.upgrade() {
                b.find_step(true);
            }
        });
        let weak = self.weak();
        self.find_entry.connect_stop_search(move |_| {
            if let Some(b) = weak.upgrade() {
                b.close_find();
            }
        });
    }

    fn open_find(self: &Rc<Self>) {
        let Some(view) = self.current_view() else { return };
        if let Some(finder) = view.find_controller() {
            let label = self.find_count.clone();
            // One handler per page is enough; WebKit keeps the finder.
            if unsafe { finder.data::<bool>("wisp-wired") }.is_none() {
                unsafe { finder.set_data("wisp-wired", true) };
                let l = label.clone();
                finder.connect_counted_matches(move |_, n| {
                    l.set_label(&if n == 0 { "No matches".to_string() } else { format!("{n} found") });
                });
                finder.connect_failed_to_find_text(move |_| label.set_label("No matches"));
            }
        }
        self.find_bar.set_reveal_child(true);
        self.find_entry.grab_focus();
        self.find_entry.select_region(0, -1);
    }

    fn find_step(&self, forward: bool) {
        if !self.find_bar.reveals_child() {
            return;
        }
        self.with_view(|v| {
            if let Some(f) = v.find_controller() {
                if forward { f.search_next() } else { f.search_previous() }
            }
        });
    }

    pub fn close_find(&self) {
        if !self.find_bar.reveals_child() {
            return;
        }
        self.find_bar.set_reveal_child(false);
        self.find_count.set_label("");
        self.with_view(|v| {
            if let Some(f) = v.find_controller() {
                f.search_finish();
            }
            v.grab_focus();
        });
    }
}
