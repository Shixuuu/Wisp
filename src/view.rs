//! Building a tab's WebView and listening to it.

use crate::browser::Browser;
use crate::curtain;
use crate::tab::{Tab, error_page};
use adw::prelude::*;
use gtk::gdk;
use std::rc::Rc;
use webkit6::prelude::*;
use webkit6::{
    LoadEvent, NavigationPolicyDecision, NavigationType, PolicyDecisionType, UserContentInjectedFrames,
    UserContentManager, UserScript, UserScriptInjectionTime, UserStyleLevel, UserStyleSheet, WebView,
};

const PICKER: &str = include_str!("js/picker.js");
pub const READER: &str = include_str!("js/reader.js");
const VEIL_HANDLER: &str = "wispVeil";

impl Browser {
    /// Build the page for a tab. `related` is the page that opened it, for a
    /// popup: the two then share a web process, as the web expects.
    pub fn make_view(self: &Rc<Self>, tab: &Rc<Tab>, related: Option<&WebView>) -> WebView {
        let content = UserContentManager::new();
        content.add_script(&UserScript::new(
            PICKER,
            UserContentInjectedFrames::TopFrame,
            UserScriptInjectionTime::End,
            &[],
            &[],
        ));
        content.register_script_message_handler(VEIL_HANDLER, None);
        let weak = self.weak();
        let id = tab.id;
        content.connect_script_message_received(Some(VEIL_HANDLER), move |_, value| {
            let Some(b) = weak.upgrade() else { return };
            let Some(json) = value.to_json(0) else { return };
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&json) {
                b.picked(id, &msg);
            }
        });

        let mut builder = WebView::builder().user_content_manager(&content).settings(&self.web.settings);
        builder = match related {
            Some(r) => builder.related_view(r),
            None if tab.private => {
                let session = self.web.private_session();
                // A fresh private session needs its downloads watched too.
                if unsafe { session.data::<bool>("wisp-downloads") }.is_none() {
                    unsafe { session.set_data("wisp-downloads", true) };
                    self.wire_private_downloads(&session);
                }
                builder.network_session(&session)
            }
            None => builder.network_session(&self.web.session),
        };
        let view = builder.build();
        view.set_zoom_level(tab.zoom.get());
        view.set_hexpand(true);
        view.set_vexpand(true);

        *tab.content.borrow_mut() = Some(content);
        *tab.view.borrow_mut() = Some(view.clone());
        self.stack.add_named(&view, Some(&tab.id.to_string()));
        self.tune(tab, &tab.url.borrow());
        self.wire_view(tab, &view);
        tab.refresh_row();
        view
    }

    /// Before each page: the ad blocker goes on or off for the site the tab is
    /// heading to, and that site's hidden elements are put in place.
    pub fn tune(&self, tab: &Tab, url: &str) {
        let Some(content) = tab.content.borrow().clone() else { return };
        let host = curtain::host_key(url).unwrap_or_default();

        content.remove_all_filters();
        let (on, paused) = {
            let p = self.prefs.borrow();
            (p.shield, p.is_paused(&host))
        };
        if on
            && !paused
            && let Some(filter) = self.web.filter.borrow().as_ref()
        {
            content.add_filter(filter);
        }

        content.remove_all_style_sheets();
        let css = self.curtain.borrow().css(&host);
        if !css.is_empty() {
            content.add_style_sheet(&UserStyleSheet::new(
                &css,
                UserContentInjectedFrames::TopFrame,
                UserStyleLevel::User,
                &[],
                &[],
            ));
        }
    }

    /// Re-tune every open page, after the blocker or a site's list changed.
    pub fn retune_all(&self) {
        for tab in self.tabs.borrow().iter() {
            let url = tab.url.borrow().clone();
            self.tune(tab, &url);
        }
    }

    fn wire_view(self: &Rc<Self>, tab: &Rc<Tab>, view: &WebView) {
        let id = tab.id;

        let weak = self.weak();
        view.connect_title_notify(move |v| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = b.tab_by_id(id) else { return };
            let title = v.title().map(|t| t.to_string()).unwrap_or_default();
            *tab.title.borrow_mut() = title.clone();
            tab.refresh_row();
            if !tab.private {
                b.history.borrow_mut().retitle(&tab.url.borrow(), &title);
            }
            if b.is_current(id) {
                b.window.set_title(Some(&tab.name()));
            }
            b.mark_session();
        });

        let weak = self.weak();
        view.connect_uri_notify(move |v| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = b.tab_by_id(id) else { return };
            let uri = v.uri().map(|u| u.to_string()).unwrap_or_default();
            *tab.url.borrow_mut() = uri;
            if b.is_current(id) {
                b.show_address();
            }
            b.mark_session();
        });

        let weak = self.weak();
        view.connect_load_changed(move |v, event| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = b.tab_by_id(id) else { return };
            let uri = v.uri().map(|u| u.to_string()).unwrap_or_default();
            match event {
                LoadEvent::Started | LoadEvent::Redirected => b.tune(&tab, &uri),
                LoadEvent::Committed => {
                    if !tab.private {
                        let title = v.title().map(|t| t.to_string()).unwrap_or_default();
                        b.history.borrow_mut().record(&uri, &title);
                    }
                }
                LoadEvent::Finished => {}
                _ => {}
            }
            if b.is_current(id) {
                b.update_progress();
            }
        });

        let weak = self.weak();
        view.connect_estimated_load_progress_notify(move |_| {
            if let Some(b) = weak.upgrade()
                && b.is_current(id)
            {
                b.update_progress();
            }
        });

        let weak = self.weak();
        view.connect_favicon_notify(move |v| {
            if let Some(tab) = weak.upgrade().and_then(|b| b.tab_by_id(id)) {
                tab.set_icon(v.favicon().as_ref());
            }
        });

        let weak = self.weak();
        view.connect_is_playing_audio_notify(move |v| {
            if let Some(tab) = weak.upgrade().and_then(|b| b.tab_by_id(id)) {
                tab.row.audio.set_visible(v.is_playing_audio() || v.is_muted());
                tab.row.audio.set_icon_name(Some(if v.is_muted() {
                    "audio-volume-muted-symbolic"
                } else {
                    "audio-volume-high-symbolic"
                }));
            }
        });

        // Middle-click, or Ctrl+click, opens a link in a background tab.
        let weak = self.weak();
        view.connect_decide_policy(move |_, decision, kind| {
            let Some(b) = weak.upgrade() else { return false };
            if kind != PolicyDecisionType::NavigationAction {
                return false;
            }
            let Some(nav) = decision.downcast_ref::<NavigationPolicyDecision>() else { return false };
            let Some(action) = nav.navigation_action() else { return false };
            if action.navigation_type() != NavigationType::LinkClicked {
                return false;
            }
            let ctrl =
                gdk::ModifierType::from_bits_truncate(action.modifiers()).contains(gdk::ModifierType::CONTROL_MASK);
            if (action.mouse_button() == 2 || ctrl)
                && let Some(uri) = action.request().and_then(|r| r.uri())
            {
                decision.ignore();
                b.open(&uri, true);
                return true;
            }
            false
        });

        // A page asking for a new window gets a new tab instead.
        let weak = self.weak();
        view.connect_create(move |v, action| {
            let b = weak.upgrade()?;
            let parent = b.tab_by_id(id)?;
            let action = action.clone();
            let uri = action.request().and_then(|r| r.uri()).map(|u| u.to_string()).unwrap_or_default();
            let tab = b.add_tab(&uri, "", false, parent.private, true);
            let child = b.make_view(&tab, Some(v));
            let weak = b.weak();
            let tab_id = tab.id;
            child.connect_ready_to_show(move |_| {
                if let Some(b) = weak.upgrade()
                    && let Some(tab) = b.tab_by_id(tab_id)
                {
                    b.select(&tab);
                }
            });
            Some(child.upcast())
        });

        let weak = self.weak();
        view.connect_close(move |_| {
            if let Some(b) = weak.upgrade() {
                b.close_tab(id);
            }
        });

        // The address of the link under the pointer, bottom left.
        let weak = self.weak();
        view.connect_mouse_target_changed(move |_, hit, _| {
            let Some(b) = weak.upgrade() else { return };
            if !b.is_current(id) {
                return;
            }
            match hit.link_uri().filter(|_| hit.context_is_link()) {
                Some(uri) => {
                    b.status.set_label(&uri);
                    b.status.set_visible(true);
                }
                None => b.status.set_visible(false),
            }
        });

        let weak = self.weak();
        view.connect_load_failed(move |v, _, uri, err| {
            // A cancelled load (you clicked elsewhere) and a load turned into a
            // download are not failures worth a page.
            if err.matches(webkit6::NetworkError::Cancelled)
                || err.matches(webkit6::PolicyError::FrameLoadInterruptedByPolicyChange)
            {
                return false;
            }
            let _ = weak.upgrade();
            v.load_alternate_html(&error_page("This page didn't load", err.message(), uri), uri, None);
            true
        });

        let weak = self.weak();
        view.connect_load_failed_with_tls_errors(move |v, uri, cert, _flags| {
            let Some(b) = weak.upgrade() else { return false };
            let host = url::Url::parse(uri).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
            v.load_alternate_html(
                &error_page(
                    "This connection isn't private",
                    "The site's certificate couldn't be verified. Someone could be reading or changing what you send.",
                    uri,
                ),
                uri,
                None,
            );
            b.ask_tls_exception(v, uri, &host, cert);
            true
        });

        let weak = self.weak();
        view.connect_permission_request(move |_, request| {
            let Some(b) = weak.upgrade() else { return false };
            b.ask_permission(id, request)
        });

        let weak = self.weak();
        view.connect_enter_fullscreen(move |_| {
            if let Some(b) = weak.upgrade() {
                b.fullscreen.set(true);
                b.set_chrome_visible(false);
                b.window.fullscreen();
            }
            true
        });
        let weak = self.weak();
        view.connect_leave_fullscreen(move |_| {
            if let Some(b) = weak.upgrade() {
                b.fullscreen.set(false);
                b.set_chrome_visible(!b.prefs.borrow().sidebar_folded);
                b.window.unfullscreen();
            }
            true
        });

        let weak = self.weak();
        view.connect_web_process_terminated(move |v, reason| {
            let Some(b) = weak.upgrade() else { return };
            if reason == webkit6::WebProcessTerminationReason::TerminatedByApi {
                return;
            }
            let uri = v.uri().map(|u| u.to_string()).unwrap_or_default();
            let why = match reason {
                webkit6::WebProcessTerminationReason::ExceededMemoryLimit => "The page used too much memory.",
                _ => "The page's process stopped unexpectedly.",
            };
            v.load_alternate_html(
                &error_page("This page crashed", &format!("{why} Reload to try again."), &uri),
                &uri,
                None,
            );
            if b.is_current(id) {
                b.toast("The page crashed");
            }
        });
    }

    pub fn is_current(&self, id: u64) -> bool {
        self.current.borrow().as_ref().is_some_and(|t| t.id == id)
    }

    pub fn update_progress(&self) {
        let Some(view) = self.current_view() else {
            self.progress.set_visible(false);
            return;
        };
        if view.is_loading() {
            self.progress.set_visible(true);
            self.progress.set_fraction(view.estimated_load_progress().max(0.08));
        } else {
            self.progress.set_visible(false);
        }
    }

    fn ask_tls_exception(self: &Rc<Self>, view: &WebView, uri: &str, host: &str, cert: &gtk::gio::TlsCertificate) {
        let dialog = adw::AlertDialog::new(
            Some("This connection isn't private"),
            Some(&format!(
                "{host} gave a certificate that couldn't be verified. Continue only if you know why, for example a device on your own network."
            )),
        );
        dialog.add_responses(&[("back", "Go Back"), ("continue", "Continue Anyway")]);
        dialog.set_response_appearance("continue", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("back"));
        dialog.set_close_response("back");
        let view = view.clone();
        let uri = uri.to_string();
        let host = host.to_string();
        let cert = cert.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "continue" {
                if let Some(session) = view.network_session() {
                    session.allow_tls_certificate_for_host(&cert, &host);
                }
                view.load_uri(&uri);
            } else if view.can_go_back() {
                view.go_back();
            }
        });
        dialog.present(Some(&self.window));
    }

    fn ask_permission(self: &Rc<Self>, id: u64, request: &webkit6::PermissionRequest) -> bool {
        use webkit6::{
            ClipboardPermissionRequest, GeolocationPermissionRequest, MediaKeySystemPermissionRequest,
            NotificationPermissionRequest, PointerLockPermissionRequest, UserMediaPermissionRequest,
            WebsiteDataAccessPermissionRequest,
        };
        let Some(tab) = self.tab_by_id(id) else { return false };
        let host = crate::address::bare_host(&tab.url.borrow()).unwrap_or_else(|| "This page".into());

        // Locking the pointer is what games do; it is undone with Esc.
        if request.is::<PointerLockPermissionRequest>() {
            request.allow();
            return true;
        }
        let what = if request.is::<GeolocationPermissionRequest>() {
            "know your location"
        } else if request.is::<NotificationPermissionRequest>() {
            "show notifications"
        } else if let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() {
            match (media.is_for_audio_device(), media.is_for_video_device()) {
                (true, true) => "use your camera and microphone",
                (true, false) => "use your microphone",
                _ => "use your camera or share your screen",
            }
        } else if request.is::<ClipboardPermissionRequest>() {
            "read your clipboard"
        } else if request.is::<MediaKeySystemPermissionRequest>() {
            "play protected content"
        } else if request.is::<WebsiteDataAccessPermissionRequest>() {
            "use cookies while embedded in another site"
        } else {
            "do something that needs your permission"
        };

        let dialog = adw::AlertDialog::new(Some(&format!("{host} wants to {what}")), None);
        dialog.add_responses(&[("deny", "Don't Allow"), ("allow", "Allow")]);
        dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
        dialog.set_close_response("deny");
        let request = request.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "allow" {
                request.allow();
            } else {
                request.deny();
            }
        });
        dialog.present(Some(&self.window));
        true
    }

    // MARK: hiding things

    pub fn start_picking(self: &Rc<Self>) {
        let Some(view) = self.current_view() else { return };
        self.picking.set(true);
        view.evaluate_javascript(
            "window.__wispVeil && window.__wispVeil.on()",
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            |_| {},
        );
        view.grab_focus();
        self.toast("Click what you want gone. Esc when you're done.");
    }

    pub fn stop_picking(&self) {
        self.picking.set(false);
        if let Some(view) = self.current_view() {
            view.evaluate_javascript(
                "window.__wispVeil && window.__wispVeil.off()",
                None,
                None,
                None::<&gtk::gio::Cancellable>,
                |_| {},
            );
        }
    }

    fn picked(self: &Rc<Self>, id: u64, msg: &serde_json::Value) {
        let Some(tab) = self.tab_by_id(id) else { return };
        if msg.get("off").is_some() {
            self.picking.set(false);
            return;
        }
        if let Some(trouble) = msg.get("trouble").and_then(|t| t.as_str()) {
            self.toast(&format!("Couldn't hide that: {trouble}"));
            return;
        }
        let Some(selector) = msg.get("selector").and_then(|s| s.as_str()) else { return };
        let label = msg.get("label").and_then(|s| s.as_str()).unwrap_or(selector);
        let note = msg.get("note").and_then(|s| s.as_str()).unwrap_or_default();
        let Some(host) = curtain::host_key(&tab.url.borrow()) else { return };
        self.curtain.borrow_mut().hide(&host, selector, label, note);
        // Take it off the page that is already showing, too.
        let url = tab.url.borrow().clone();
        self.tune(&tab, &url);
        if let Some(view) = tab.view.borrow().as_ref() {
            let quoted = serde_json::to_string(selector).unwrap_or_default();
            let js = format!(
                "(function(){{try{{document.querySelectorAll({quoted}).forEach(function(e){{e.style.setProperty('display','none','important')}})}}catch(e){{}}}})()"
            );
            view.evaluate_javascript(&js, None, None, None::<&gtk::gio::Cancellable>, |_| {});
        }
        let toast = adw::Toast::new(&format!("Hid {label}"));
        toast.set_button_label(Some("Undo"));
        let weak = self.weak();
        toast.connect_button_clicked(move |_| {
            if let Some(b) = weak.upgrade() {
                b.curtain.borrow_mut().undo(&host);
                if let Some(view) = b.tab_by_id(id).and_then(|t| t.view.borrow().clone()) {
                    view.reload();
                }
            }
        });
        self.toasts.add_toast(toast);
    }

    pub fn read(&self) {
        let Some(view) = self.current_view() else { return };
        let weak_toast = self.toasts.clone();
        view.evaluate_javascript(READER, None, None, None::<&gtk::gio::Cancellable>, move |result| {
            if let Ok(value) = result
                && value.to_str() == "none"
            {
                weak_toast.add_toast(adw::Toast::new("Nothing on this page reads like an article"));
            }
        });
    }
}
