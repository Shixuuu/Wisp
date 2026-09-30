//! Everything WebKit-wide: the network sessions, the page settings, and the
//! compiled ad-block list. One of each for the browser; tabs borrow them.

use crate::settings::Prefs;
use crate::{shield, store};
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;
use webkit6::{CacheModel, HardwareAccelerationPolicy, NetworkSession, UserContentFilter, UserContentFilterStore};

pub struct Web {
    /// Cookies, site data and cache on disk, for ordinary tabs.
    pub session: NetworkSession,
    /// Made with the first private tab, in memory only, dropped with the last.
    shy: RefCell<Option<NetworkSession>>,
    pub settings: webkit6::Settings,
    /// None until compiled, a moment after the first launch.
    pub filter: RefCell<Option<UserContentFilter>>,
}

impl Web {
    pub fn new(prefs: &Prefs) -> Web {
        let data = store::data_dir().join("webkit");
        let cache = store::cache_dir().join("webkit");
        let session = NetworkSession::new(data.to_str(), cache.to_str());
        session.set_persistent_credential_storage_enabled(true);
        prepare(&session, !prefs.keeps_sign_ins);
        if let Some(context) = webkit6::WebContext::default() {
            context.set_cache_model(CacheModel::WebBrowser);
            context.set_spell_checking_enabled(true);
        }
        let settings = webkit6::Settings::new();
        apply(&settings, prefs);
        Web { session, shy: RefCell::default(), settings, filter: RefCell::default() }
    }

    /// The private session, and whether it was just made.
    pub fn shy_session(&self) -> (NetworkSession, bool) {
        let mut shy = self.shy.borrow_mut();
        if let Some(s) = shy.as_ref() {
            return (s.clone(), false);
        }
        let s = NetworkSession::new_ephemeral();
        // Private tabs keep tracking prevention on whatever the setting says.
        prepare(&s, true);
        *shy = Some(s.clone());
        (s, true)
    }

    pub fn drop_shy_session(&self) {
        self.shy.borrow_mut().take();
    }

    pub fn set_tracking_prevention(&self, on: bool) {
        self.session.set_itp_enabled(on);
    }

    /// Compile the block list into WebKit's bytecode, kept in the cache.
    pub fn compile_shield(self: &Rc<Self>, done: impl FnOnce(Result<(), String>) + 'static) {
        let dir = store::cache_dir().join("filters");
        let _ = std::fs::create_dir_all(&dir);
        let filters = UserContentFilterStore::new(&dir.to_string_lossy());
        let source = glib::Bytes::from_owned(shield::rules().into_bytes());
        let web = Rc::downgrade(self);
        filters.save(shield::IDENTIFIER, &source, None::<&gtk::gio::Cancellable>, move |result| match result {
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

fn prepare(session: &NetworkSession, tracking_prevention: bool) {
    session.set_itp_enabled(tracking_prevention);
    if let Some(manager) = session.website_data_manager() {
        manager.set_favicons_enabled(true);
    }
}

pub fn apply(s: &webkit6::Settings, prefs: &Prefs) {
    s.set_enable_developer_extras(true);
    s.set_enable_smooth_scrolling(true);
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
    s.set_javascript_can_open_windows_automatically(false);
}
