//! One tab: an address, a title, and, only once you open it, a web page.
//!
//! A tab restored from last session, or one that has gone to sleep, is its
//! address and title and nothing else; its page is built the first time it
//! is shown. That is why forty tabs cost nothing at launch.

use crate::address;
use crate::browser::Browser;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;
use webkit6::prelude::*;
use webkit6::{
    LoadEvent, NavigationPolicyDecision, NavigationType, PolicyDecisionType, UserContentInjectedFrames,
    UserContentManager, UserScript, UserScriptInjectionTime, UserStyleLevel, UserStyleSheet, WebView,
};

/// The isolated world the veil and the meter talk in. A page's own scripts
/// do not see it, and cannot post into it.
pub const SCRIPT_WORLD: &str = "wisp";

const PICKER: &str = include_str!("js/picker.js");
const READER: &str = include_str!("js/reader.js");
const METER: &str = include_str!("js/meter.js");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    Title,
    Address,
    Loading,
    Icon,
    Sound,
    Failure,
    Meter,
    Sleep,
}

pub struct Tab {
    pub id: u64,
    /// A private tab: keeps nothing, writes nothing down.
    pub shy: bool,
    pub url: RefCell<Option<String>>,
    pub title: RefCell<String>,
    /// A name you gave it, over whatever the page calls itself.
    pub name: RefCell<Option<String>>,
    /// The letter a pinned tab is reduced to; None when not pinned.
    pub pin: RefCell<Option<String>>,
    /// Where a pinned tab goes back to.
    pub home: RefCell<Option<String>>,
    pub loading: Cell<bool>,
    pub noisy: Cell<bool>,
    pub muted: Cell<bool>,
    pub reading: Cell<bool>,
    /// How far down the page, 0 to 1.
    pub meter: Cell<f64>,
    pub icon: RefCell<Option<gdk::Texture>>,
    pub failure: RefCell<Option<String>>,
    pub view: RefCell<Option<WebView>>,
    content: RefCell<Option<UserContentManager>>,
    /// Back and forward, kept while it sleeps.
    state: RefCell<Option<glib::Bytes>>,
    pub touched: Cell<Instant>,
    pub zoom: Cell<f64>,
    pub preview: RefCell<Option<gdk::Texture>>,
    /// What was typed on a blank tab, kept when you step away from it.
    pub draft: RefCell<String>,
    /// Host, whether the shield is on, and the veil stylesheet last installed.
    tuned: RefCell<Option<(String, bool, String)>>,
}

impl Tab {
    pub fn new(id: u64, url: Option<String>, title: &str, shy: bool) -> Tab {
        Tab {
            id,
            shy,
            url: RefCell::new(url),
            title: RefCell::new(title.to_string()),
            name: RefCell::default(),
            pin: RefCell::default(),
            home: RefCell::default(),
            loading: Cell::new(false),
            noisy: Cell::new(false),
            muted: Cell::new(false),
            reading: Cell::new(false),
            meter: Cell::new(0.0),
            icon: RefCell::default(),
            failure: RefCell::default(),
            view: RefCell::default(),
            content: RefCell::default(),
            state: RefCell::default(),
            touched: Cell::new(Instant::now()),
            zoom: Cell::new(1.0),
            preview: RefCell::default(),
            draft: RefCell::default(),
            tuned: RefCell::default(),
        }
    }

    /// A tab that has never been anywhere shows the address field instead.
    pub fn is_blank(&self) -> bool {
        self.url.borrow().is_none()
    }

    pub fn asleep(&self) -> bool {
        !self.is_blank() && self.view.borrow().is_none()
    }

    pub fn address(&self) -> String {
        self.url.borrow().clone().unwrap_or_default()
    }

    /// The name you gave it, the page's title, or its address until it has one.
    pub fn label(&self) -> String {
        if let Some(name) = self.name.borrow().as_ref() {
            return name.clone();
        }
        let title = self.title.borrow();
        if !title.trim().is_empty() {
            return title.trim().to_string();
        }
        match self.url.borrow().as_ref() {
            Some(url) => address::pretty(url),
            None => "New Tab".into(),
        }
    }

    /// The letter a pinned tab shows, and what stands in for a missing icon.
    pub fn monogram(&self) -> String {
        let url = self.address();
        let host = address::bare_host(&url).unwrap_or_else(|| url.rsplit('/').next().unwrap_or("").to_string());
        host.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "•".into())
    }

    pub fn host(&self) -> Option<String> {
        address::bare_host(&self.address())
    }

    pub fn touch(&self) {
        self.touched.set(Instant::now());
    }

    pub fn js(&self, script: &str) {
        self.js_in(script, None);
    }

    pub fn js_in(&self, script: &str, world: Option<&str>) {
        if let Some(view) = self.view.borrow().as_ref() {
            view.evaluate_javascript(script, world, None, None::<&gio::Cancellable>, |_| {});
        }
    }
}

impl Browser {
    /// Build a tab's page. `related`: the page that opened it, for a popup,
    /// which then shares its process as the web expects.
    pub fn build(self: &Rc<Self>, tab: &Rc<Tab>, related: Option<&WebView>) -> WebView {
        let content = UserContentManager::new();
        let at_end = |source| {
            UserScript::for_world(
                source,
                UserContentInjectedFrames::TopFrame,
                UserScriptInjectionTime::End,
                SCRIPT_WORLD,
                &[],
                &[],
            )
        };
        content.add_script(&at_end(PICKER));
        content.add_script(&at_end(METER));
        for name in ["wispVeil", "wispMeter"] {
            content.register_script_message_handler(name, Some(SCRIPT_WORLD));
            let weak = self.weak();
            let id = tab.id;
            content.connect_script_message_received(Some(name), move |_, value| {
                let (Some(b), Some(json)) = (weak.upgrade(), value.to_json(0)) else { return };
                let Some(tab) = b.tab(id) else { return };
                let Ok(msg) = serde_json::from_str::<serde_json::Value>(&json) else { return };
                if let Some(meter) = msg.as_f64() {
                    tab.meter.set(meter);
                    b.changed(&tab, Change::Meter);
                } else {
                    b.picked(&tab, &msg);
                }
            });
        }

        let mut builder = WebView::builder().user_content_manager(&content).settings(&self.web.settings);
        builder = match related {
            Some(r) => builder.related_view(r),
            None if tab.shy => {
                let (session, fresh) = self.web.shy_session();
                if fresh {
                    self.watch_downloads(&session);
                }
                builder.network_session(&session)
            }
            None => builder.network_session(&self.web.session),
        };
        let view = builder.build();
        view.set_zoom_level(tab.zoom.get());
        *tab.content.borrow_mut() = Some(content);
        *tab.view.borrow_mut() = Some(view.clone());
        self.tune(tab, &tab.address(), None);
        self.listen(tab, &view);
        view
    }

    /// Wake a sleeping tab: its page built again, back where it was.
    pub fn wake(self: &Rc<Self>, tab: &Rc<Tab>) {
        if tab.view.borrow().is_some() || tab.is_blank() {
            return;
        }
        let view = self.build(tab, None);
        let restored = tab.state.take().map(|bytes| {
            let state = webkit6::WebViewSessionState::new(&bytes);
            view.restore_session_state(&state);
            view.back_forward_list().and_then(|l| l.current_item())
        });
        match restored.flatten() {
            Some(item) => view.go_to_back_forward_list_item(&item),
            None => view.load_uri(&tab.address()),
        }
        self.stage_add(tab);
        self.changed(tab, Change::Sleep);
    }

    /// Give a tab's memory back; its address, title and history stay.
    pub fn sleep(&self, tab: &Tab) {
        let Some(view) = tab.view.take() else { return };
        *tab.state.borrow_mut() = view.session_state().and_then(|s| s.serialize());
        // Released, not closed: closing would run the page's own close,
        // which closes the tab.
        self.stage_remove(&view);
        self.panes_unwatch(tab.id);
        tab.tuned.take();
        tab.content.take();
        tab.loading.set(false);
        tab.noisy.set(false);
        tab.reading.set(false);
    }

    /// Before each page: the ad blocker on or off for the site the tab is
    /// heading to, and that site's hidden things put back before it draws.
    /// `spared`: one hidden thing shown for a moment (the Hidden panel's peek).
    pub fn tune(&self, tab: &Tab, url: &str, spared: Option<&str>) {
        let Some(content) = tab.content.borrow().clone() else { return };
        let host = address::bare_host(url).unwrap_or_default();
        let blocking = {
            let p = self.prefs.borrow();
            p.shielded && !p.is_paused(&host)
        };
        let css = self.curtain.borrow().css_without(&host, spared);
        let key = (host, blocking, css.clone());
        if tab.tuned.borrow().as_ref() == Some(&key) {
            return;
        }
        *tab.tuned.borrow_mut() = Some(key);
        content.remove_all_filters();
        if let (true, Some(filter)) = (blocking, self.web.filter.borrow().as_ref()) {
            content.add_filter(filter);
        }
        content.remove_all_style_sheets();
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

    pub fn retune(&self) {
        for tab in self.tabs.borrow().iter() {
            tab.tuned.take();
            self.tune(tab, &tab.address(), None);
        }
    }

    /// Reading mode on, or off again (the page as it was).
    pub fn toggle_reader(self: &Rc<Self>) {
        let Some(tab) = self.active() else { return };
        let Some(view) = tab.view.borrow().clone() else { return };
        if tab.reading.replace(false) {
            view.reload();
            return;
        }
        let weak = self.weak();
        let id = tab.id;
        view.evaluate_javascript(READER, None, None, None::<&gio::Cancellable>, move |result| {
            let (Some(b), Some(tab)) = (weak.upgrade(), weak.upgrade().and_then(|b| b.tab(id))) else { return };
            match result.map(|v| v.to_str().to_string()).as_deref() {
                Ok("read") => tab.reading.set(true),
                _ => b.announce("Nothing to read on this page"),
            }
        });
    }

    fn listen(self: &Rc<Self>, tab: &Rc<Tab>, view: &WebView) {
        let id = tab.id;
        let on = move |b: &Rc<Browser>| b.tab(id);

        let weak = self.weak();
        view.connect_title_notify(move |v| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = on(&b) else { return };
            *tab.title.borrow_mut() = v.title().map(|t| t.to_string()).unwrap_or_default();
            if !tab.shy {
                b.history.borrow_mut().retitle(&tab.address(), &tab.title.borrow());
            }
            b.changed(&tab, Change::Title);
        });

        let weak = self.weak();
        view.connect_uri_notify(move |v| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = on(&b) else { return };
            let uri = v.uri().map(|u| u.to_string()).filter(|u| !u.is_empty());
            if uri.is_some() {
                *tab.url.borrow_mut() = uri;
            }
            tab.reading.set(false);
            b.changed(&tab, Change::Address);
        });

        let weak = self.weak();
        view.connect_load_changed(move |v, event| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = on(&b) else { return };
            let uri = v.uri().map(|u| u.to_string()).unwrap_or_default();
            match event {
                LoadEvent::Started | LoadEvent::Redirected => {
                    b.tune(&tab, &uri, None);
                    if tab.failure.take().is_some() {
                        b.changed(&tab, Change::Failure);
                    }
                }
                LoadEvent::Committed => {
                    if !tab.shy {
                        b.history.borrow_mut().record(&uri, &v.title().map(|t| t.to_string()).unwrap_or_default());
                    }
                    let zoom = b.zoom_for(&uri);
                    tab.zoom.set(zoom);
                    v.set_zoom_level(zoom);
                    tab.meter.set(0.0);
                }
                _ => {}
            }
        });

        let weak = self.weak();
        view.connect_is_loading_notify(move |v| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = on(&b) else { return };
            tab.loading.set(v.is_loading());
            b.changed(&tab, Change::Loading);
        });

        let weak = self.weak();
        view.connect_favicon_notify(move |v| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = on(&b) else { return };
            *tab.icon.borrow_mut() = v.favicon();
            b.changed(&tab, Change::Icon);
        });

        let weak = self.weak();
        view.connect_is_playing_audio_notify(move |v| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = on(&b) else { return };
            tab.noisy.set(v.is_playing_audio());
            b.changed(&tab, Change::Sound);
        });

        // Middle-click, or Ctrl+click, opens a link beside this tab, behind it.
        let weak = self.weak();
        view.connect_decide_policy(move |_, decision, kind| {
            let Some(b) = weak.upgrade() else { return false };
            let Some(tab) = on(&b) else { return false };
            if kind != PolicyDecisionType::NavigationAction {
                return false;
            }
            let Some(nav) = decision.downcast_ref::<NavigationPolicyDecision>() else { return false };
            let Some(action) = nav.navigation_action() else { return false };
            let mods = gdk::ModifierType::from_bits_truncate(action.modifiers());
            if action.navigation_type() == NavigationType::LinkClicked
                && (action.mouse_button() == 2 || mods.contains(gdk::ModifierType::CONTROL_MASK))
                && let Some(uri) = action.request().and_then(|r| r.uri())
            {
                decision.ignore();
                b.open(&uri, mods.contains(gdk::ModifierType::SHIFT_MASK), Some(&tab));
                return true;
            }
            false
        });

        // A page asking for a new window gets a new tab beside it.
        let weak = self.weak();
        view.connect_create(move |v, action| {
            let b = weak.upgrade()?;
            let parent = on(&b)?;
            let uri = action.clone().request().and_then(|r| r.uri()).map(|u| u.to_string()).filter(|u| !u.is_empty());
            let tab = b.insert(uri, "", parent.shy, Some(&parent));
            let child = b.build(&tab, Some(v));
            b.stage_add(&tab);
            let weak = b.weak();
            let id = tab.id;
            child.connect_ready_to_show(move |_| {
                if let Some(b) = weak.upgrade()
                    && let Some(tab) = b.tab(id)
                {
                    b.select(&tab);
                }
            });
            Some(child.upcast())
        });

        let weak = self.weak();
        view.connect_close(move |_| {
            if let Some(b) = weak.upgrade()
                && let Some(tab) = on(&b)
            {
                b.close(&tab);
            }
        });

        let weak = self.weak();
        view.connect_mouse_target_changed(move |_, hit, _| {
            let Some(b) = weak.upgrade() else { return };
            let link = hit.link_uri().filter(|_| hit.context_is_link()).map(|u| u.to_string());
            if b.active().is_some_and(|t| t.id == id) {
                b.hover_link(link);
            }
        });

        let weak = self.weak();
        view.connect_load_failed(move |_, _, _, err| {
            if err.matches(webkit6::NetworkError::Cancelled)
                || err.matches(webkit6::PolicyError::FrameLoadInterruptedByPolicyChange)
            {
                return false;
            }
            let Some(b) = weak.upgrade() else { return false };
            let Some(tab) = on(&b) else { return false };
            *tab.failure.borrow_mut() = Some(err.message().to_string());
            b.changed(&tab, Change::Failure);
            true
        });

        let weak = self.weak();
        view.connect_load_failed_with_tls_errors(move |_, uri, _, _| {
            let Some(b) = weak.upgrade() else { return false };
            let Some(tab) = on(&b) else { return false };
            let host = address::bare_host(uri).unwrap_or_default();
            *tab.failure.borrow_mut() =
                Some(format!("{host}'s certificate couldn't be checked, so the page wasn't shown"));
            b.changed(&tab, Change::Failure);
            true
        });

        let weak = self.weak();
        view.connect_permission_request(move |_, request| {
            let Some(b) = weak.upgrade() else { return false };
            let Some(tab) = on(&b) else { return false };
            b.ask(&tab, request)
        });

        let weak = self.weak();
        view.connect_enter_fullscreen(move |_| {
            if let Some(b) = weak.upgrade() {
                b.immerse(true);
            }
            false
        });
        let weak = self.weak();
        view.connect_leave_fullscreen(move |_| {
            if let Some(b) = weak.upgrade() {
                b.immerse(false);
            }
            false
        });

        let weak = self.weak();
        view.connect_web_process_terminated(move |_, reason| {
            let Some(b) = weak.upgrade() else { return };
            let Some(tab) = on(&b) else { return };
            if reason == webkit6::WebProcessTerminationReason::TerminatedByApi {
                return;
            }
            *tab.failure.borrow_mut() = Some(match reason {
                webkit6::WebProcessTerminationReason::ExceededMemoryLimit => "This page used too much memory".into(),
                _ => "This page stopped working".into(),
            });
            b.changed(&tab, Change::Failure);
        });
    }

    /// A small picture of the page, for the tab switcher.
    pub fn capture(&self, tab: &Rc<Tab>) {
        let Some(view) = tab.view.borrow().clone() else { return };
        let tab = Rc::downgrade(tab);
        let window = self.window.clone();
        view.snapshot(
            webkit6::SnapshotRegion::Visible,
            webkit6::SnapshotOptions::NONE,
            None::<&gio::Cancellable>,
            move |result| {
                if let (Ok(texture), Some(tab)) = (result, tab.upgrade()) {
                    *tab.preview.borrow_mut() = Some(shrink(&window, &texture).unwrap_or(texture));
                }
            },
        );
    }
}

/// The switcher shows these a few hundred pixels wide. A snapshot is the size
/// of the whole page area, several megabytes each, kept for every tab.
const PREVIEW_WIDTH: f32 = 480.0;

fn shrink(window: &impl IsA<gtk::Widget>, texture: &gdk::Texture) -> Option<gdk::Texture> {
    let (w, h) = (texture.width() as f32, texture.height() as f32);
    if w <= PREVIEW_WIDTH {
        return None;
    }
    let scale = PREVIEW_WIDTH / w;
    let snapshot = gtk::Snapshot::new();
    snapshot.scale(scale, scale);
    snapshot.append_texture(texture, &gtk::graphene::Rect::new(0.0, 0.0, w, h));
    let node = snapshot.to_node()?;
    let renderer = window.native()?.renderer()?;
    Some(renderer.render_texture(&node, Some(&gtk::graphene::Rect::new(0.0, 0.0, w * scale, h * scale))))
}
