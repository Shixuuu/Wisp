//! Settings, in one window. Every change applies at once and is saved.

use crate::browser::Browser;
use crate::settings::{Engine, Theme};
use adw::prelude::*;
use gtk::glib;
use std::rc::Rc;

impl Browser {
    pub fn start_shield(self: &Rc<Self>) {
        let weak = self.weak();
        self.web.compile_shield(move |result| {
            let Some(b) = weak.upgrade() else { return };
            match result {
                // Tabs that opened while it was compiling get it now.
                Ok(()) => b.retune_all(),
                Err(err) => b.toast(&format!("The ad blocker couldn't start: {err}")),
            }
        });
    }

    pub fn show_settings(self: &Rc<Self>) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Settings");

        // General.
        let page = adw::PreferencesPage::builder().title("General").icon_name("preferences-system-symbolic").build();

        let search = adw::PreferencesGroup::builder().title("Search").build();
        let names: Vec<&str> = Engine::ALL.iter().map(|e| e.name()).collect();
        let engine = adw::ComboRow::builder().title("Search engine").model(&gtk::StringList::new(&names)).build();
        let current = self.prefs.borrow().engine;
        engine.set_selected(Engine::ALL.iter().position(|e| *e == current).unwrap_or(0) as u32);
        let custom = adw::EntryRow::builder().title("Custom engine address, with %s for the words").build();
        custom.set_text(&self.prefs.borrow().custom_engine);
        custom.set_visible(current == Engine::Custom);
        let weak = self.weak();
        let c = custom.clone();
        engine.connect_selected_notify(move |row| {
            let Some(b) = weak.upgrade() else { return };
            let chosen = Engine::ALL[row.selected() as usize];
            b.prefs.borrow_mut().engine = chosen;
            b.prefs.borrow().save();
            c.set_visible(chosen == Engine::Custom);
        });
        let weak = self.weak();
        custom.connect_changed(move |row| {
            let Some(b) = weak.upgrade() else { return };
            let text = row.text().to_string();
            let ok = crate::settings::accepts(&text);
            if ok || text.is_empty() {
                row.remove_css_class("error");
            } else {
                row.add_css_class("error");
            }
            b.prefs.borrow_mut().custom_engine = text;
            b.prefs.borrow().save();
        });
        let keywords = adw::ActionRow::builder()
            .title("Keywords")
            .subtitle(keyword_summary(&self.prefs.borrow().keywords))
            .build();
        keywords.set_subtitle_lines(3);
        search.add(&engine);
        search.add(&custom);
        search.add(&keywords);
        page.add(&search);

        let look = adw::PreferencesGroup::builder().title("Look").build();
        let theme = adw::ComboRow::builder()
            .title("Appearance")
            .model(&gtk::StringList::new(&["Follow the system", "Light", "Dark"]))
            .build();
        theme.set_selected(match self.prefs.borrow().theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        });
        let weak = self.weak();
        theme.connect_selected_notify(move |row| {
            if let Some(b) = weak.upgrade() {
                b.set_theme(match row.selected() {
                    1 => Theme::Light,
                    2 => Theme::Dark,
                    _ => Theme::System,
                });
            }
        });
        let left = adw::SwitchRow::builder()
            .title("Tabs down the left")
            .subtitle("Off puts them across the top (Ctrl+Shift+S)")
            .active(self.prefs.borrow().tabs_on_left)
            .build();
        let weak = self.weak();
        left.connect_active_notify(move |row| {
            if let Some(b) = weak.upgrade() {
                b.prefs.borrow_mut().tabs_on_left = row.is_active();
                b.prefs.borrow().save();
                b.arrange();
            }
        });
        look.add(&theme);
        look.add(&left);
        page.add(&look);

        let tabs = adw::PreferencesGroup::builder().title("Tabs").build();
        let restore = adw::SwitchRow::builder()
            .title("Reopen last session's tabs")
            .subtitle("Pinned tabs always come back")
            .active(self.prefs.borrow().restore_session)
            .build();
        let weak = self.weak();
        restore.connect_active_notify(move |row| {
            if let Some(b) = weak.upgrade() {
                b.prefs.borrow_mut().restore_session = row.is_active();
                b.prefs.borrow().save();
            }
        });
        let sleep = adw::SpinRow::with_range(0.0, 720.0, 5.0);
        sleep.set_title("Put background tabs to sleep after (minutes)");
        sleep.set_subtitle("A sleeping tab gives its memory back and reloads when you open it. 0 = never.");
        sleep.set_value(self.prefs.borrow().sleep_minutes as f64);
        let weak = self.weak();
        sleep.connect_value_notify(move |row| {
            if let Some(b) = weak.upgrade() {
                b.prefs.borrow_mut().sleep_minutes = row.value() as u32;
                b.prefs.borrow().save();
            }
        });
        tabs.add(&restore);
        tabs.add(&sleep);
        page.add(&tabs);
        dialog.add(&page);

        // Privacy.
        let privacy = adw::PreferencesPage::builder().title("Privacy").icon_name("security-high-symbolic").build();
        let blocking = adw::PreferencesGroup::builder()
            .title("Ads and trackers")
            .description("Third-party ad networks and trackers are stopped inside WebKit's networking, before a request is made.")
            .build();
        let shield =
            adw::SwitchRow::builder().title("Block ads and trackers").active(self.prefs.borrow().shield).build();
        let weak = self.weak();
        shield.connect_active_notify(move |row| {
            if let Some(b) = weak.upgrade() {
                b.prefs.borrow_mut().shield = row.is_active();
                b.prefs.borrow().save();
                b.retune_all();
            }
        });
        blocking.add(&shield);
        let current_host = self.current.borrow().as_ref().and_then(|t| crate::curtain::host_key(&t.url.borrow()));
        if let Some(host) = current_host {
            let site = adw::SwitchRow::builder()
                .title(format!("Block on {host}"))
                .subtitle("Turn off for a site the blocker breaks")
                .active(!self.prefs.borrow().is_paused(&host))
                .build();
            let weak = self.weak();
            site.connect_active_notify(move |row| {
                if let Some(b) = weak.upgrade() {
                    b.prefs.borrow_mut().set_paused(&host, !row.is_active());
                    b.prefs.borrow().save();
                    b.retune_all();
                    if let Some(v) = b.current_view() {
                        use webkit6::prelude::*;
                        v.reload();
                    }
                }
            });
            blocking.add(&site);
        }
        let paused = self.prefs.borrow().shield_paused.clone();
        if !paused.is_empty() {
            let list = adw::ExpanderRow::builder().title("Sites the blocker is off for").build();
            for host in paused {
                let row = adw::ActionRow::builder().title(&host).build();
                let button = gtk::Button::from_icon_name("user-trash-symbolic");
                button.add_css_class("flat");
                button.set_valign(gtk::Align::Center);
                let weak = self.weak();
                let r = row.clone();
                button.connect_clicked(move |_| {
                    if let Some(b) = weak.upgrade() {
                        b.prefs.borrow_mut().set_paused(&host, false);
                        b.prefs.borrow().save();
                        b.retune_all();
                        r.set_visible(false);
                    }
                });
                row.add_suffix(&button);
                list.add_row(&row);
            }
            blocking.add(&list);
        }
        privacy.add(&blocking);

        let data = adw::PreferencesGroup::builder()
            .title("Your data")
            .description(
                "History, bookmarks and open tabs are small files in ~/.local/share/wisp. Nothing is sent anywhere.",
            )
            .build();
        let clear_site = adw::ActionRow::builder().title("Clear Cookies and Site Data…").activatable(true).build();
        clear_site.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let weak = self.weak();
        clear_site.connect_activated(move |row| {
            let Some(b) = weak.upgrade() else { return };
            let dialog = adw::AlertDialog::new(
                Some("Clear cookies and site data?"),
                Some("You will be signed out of websites. History and bookmarks stay."),
            );
            dialog.add_responses(&[("cancel", "Cancel"), ("clear", "Clear")]);
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let weak = b.weak();
            dialog.connect_response(Some("clear"), move |_, _| {
                let Some(b) = weak.upgrade() else { return };
                if let Some(manager) = b.web.session.website_data_manager() {
                    // WebKit calls back on this thread; the weak reference
                    // only needs to be sendable to satisfy the binding.
                    let toasts: glib::SendWeakRef<adw::ToastOverlay> = b.toasts.downgrade().into();
                    manager.clear(
                        webkit6::WebsiteDataTypes::ALL,
                        glib::TimeSpan::from_seconds(0),
                        None::<&gtk::gio::Cancellable>,
                        move |result| {
                            if let (Ok(()), Some(toasts)) = (result, toasts.upgrade()) {
                                toasts.add_toast(adw::Toast::new("Cookies and site data cleared"));
                            }
                        },
                    );
                }
            });
            dialog.present(Some(row));
        });
        data.add(&clear_site);
        privacy.add(&data);
        dialog.add(&privacy);

        // Performance.
        let perf = adw::PreferencesPage::builder().title("Performance").icon_name("speedometer-symbolic").build();
        let render = adw::PreferencesGroup::builder()
            .title("Rendering")
            .description("Takes effect for pages opened after the change.")
            .build();
        let gpu = adw::SwitchRow::builder()
            .title("Hardware acceleration")
            .subtitle("Draw pages on the GPU. Turn off if a graphics driver misbehaves.")
            .active(self.prefs.borrow().hardware_acceleration)
            .build();
        let weak = self.weak();
        gpu.connect_active_notify(move |row| {
            if let Some(b) = weak.upgrade() {
                b.prefs.borrow_mut().hardware_acceleration = row.is_active();
                b.prefs.borrow().save();
                crate::web::apply_settings(&b.web.settings, &b.prefs.borrow());
            }
        });
        let smooth =
            adw::SwitchRow::builder().title("Smooth scrolling").active(self.prefs.borrow().smooth_scrolling).build();
        let weak = self.weak();
        smooth.connect_active_notify(move |row| {
            if let Some(b) = weak.upgrade() {
                b.prefs.borrow_mut().smooth_scrolling = row.is_active();
                b.prefs.borrow().save();
                crate::web::apply_settings(&b.web.settings, &b.prefs.borrow());
            }
        });
        render.add(&gpu);
        render.add(&smooth);
        perf.add(&render);
        dialog.add(&perf);

        dialog.present(Some(&self.window));
    }
}

fn keyword_summary(keywords: &[crate::settings::Keyword]) -> String {
    let list: Vec<String> = keywords.iter().map(|k| k.keyword.clone()).collect();
    format!("Type a keyword, a space, then words: {}. Edit them in ~/.config/wisp/settings.json.", list.join(", "))
}
