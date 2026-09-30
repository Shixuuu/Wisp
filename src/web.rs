//! Everything WebKit-wide: the network sessions, the page settings, and the
//! compiled ad-block list. One of each for the whole browser; tabs borrow.

use crate::settings::Settings;
use crate::{shield, store};
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;
use webkit6::{
    CacheModel, HardwareAccelerationPolicy, NetworkSession, UserContentFilter, UserContentFilterStore, WebContext,
};

pub struct Web {
    /// Cookies, site data and cache on disk, for ordinary tabs.
    pub session: NetworkSession,
    /// Made the first time a private tab opens; lives in memory only and is
    /// shared by every private tab until the last one closes.
    private: RefCell<Option<NetworkSession>>,
    pub settings: webkit6::Settings,
    /// None until compiled (it takes a moment at first launch).
    pub filter: RefCell<Option<UserContentFilter>>,
}

impl Web {
    pub fn new(prefs: &Settings) -> Web {
        let data = store::data_dir().join("webkit");
        let cache = store::cache_dir().join("webkit");
        let session = NetworkSession::new(data.to_str(), cache.to_str());
        prepare_session(&session);
        // Remember logins that sites ask for with HTTP authentication.
        session.set_persistent_credential_storage_enabled(true);

        if let Some(context) = WebContext::default() {
            // Aggressive memory and disk caching, as a browser wants (as
            // opposed to a help viewer or a single-document app).
            context.set_cache_model(CacheModel::WebBrowser);
            context.set_spell_checking_enabled(true);
            if !prefs.spell_languages.is_empty() {
                let langs: Vec<&str> = prefs.spell_languages.iter().map(String::as_str).collect();
                context.set_spell_checking_languages(&langs);
            }
        }

        let settings = webkit6::Settings::new();
        apply_settings(&settings, prefs);

        Web { session, private: RefCell::new(None), settings, filter: RefCell::new(None) }
    }

    pub fn private_session(&self) -> NetworkSession {
        self.private
            .borrow_mut()
            .get_or_insert_with(|| {
                let session = NetworkSession::new_ephemeral();
                prepare_session(&session);
                session
            })
            .clone()
    }

    /// Called when the last private tab closes: the next private tab starts
    /// with an empty cookie jar.
    pub fn drop_private_session(&self) {
        self.private.borrow_mut().take();
    }

    /// Compile the block list into WebKit's bytecode. `done` runs on the main
    /// thread once it is ready, or with an error message if it isn't.
    pub fn compile_shield(self: &Rc<Self>, done: impl FnOnce(Result<(), String>) + 'static) {
        let dir = store::cache_dir().join("filters");
        let _ = std::fs::create_dir_all(&dir);
        let Some(path) = dir.to_str() else {
            done(Err("cache directory isn't valid UTF-8".into()));
            return;
        };
        let filter_store = UserContentFilterStore::new(path);
        let source = glib::Bytes::from_owned(shield::rules().into_bytes());
        let web = Rc::downgrade(self);
        filter_store.save(shield::IDENTIFIER, &source, None::<&gtk::gio::Cancellable>, move |result| match result {
            Ok(filter) => {
                if let Some(web) = web.upgrade() {
                    *web.filter.borrow_mut() = Some(filter);
                }
                done(Ok(()));
            }
            Err(err) => done(Err(err.to_string())),
        });
    }
}

fn prepare_session(session: &NetworkSession) {
    // Intelligent Tracking Prevention: WebKit's own defence against
    // cross-site tracking cookies, as in Safari.
    session.set_itp_enabled(true);
    if let Some(manager) = session.website_data_manager() {
        manager.set_favicons_enabled(true);
    }
}

pub fn apply_settings(s: &webkit6::Settings, prefs: &Settings) {
    s.set_enable_developer_extras(true);
    s.set_enable_smooth_scrolling(prefs.smooth_scrolling);
    s.set_enable_back_forward_navigation_gestures(true);
    s.set_hardware_acceleration_policy(if prefs.hardware_acceleration {
        HardwareAccelerationPolicy::Always
    } else {
        HardwareAccelerationPolicy::Never
    });
    s.set_enable_webgl(true);
    s.set_enable_webaudio(true);
    s.set_enable_media_stream(true);
    s.set_enable_webrtc(true);
    s.set_enable_mediasource(true);
    s.set_enable_encrypted_media(true);
    s.set_enable_page_cache(true);
    s.set_enable_fullscreen(true);
    s.set_enable_html5_local_storage(true);
    s.set_enable_html5_database(true);
    s.set_enable_site_specific_quirks(true);
    // Popups only when a person clicked for them.
    s.set_javascript_can_open_windows_automatically(false);
    // The user agent is left as WebKitGTK's own: it is tuned so sites treat
    // it like Safari, and anything appended to it only invites sniffing.
}
