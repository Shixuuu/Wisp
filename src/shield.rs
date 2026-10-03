//! The ad and tracker blocker.
//!
//! WebKitGTK speaks the same content-blocker JSON as Safari's
//! `WKContentRuleList`, so the rules are the ones Search ships, unchanged.
//! The list is compiled once into WebKit's own bytecode (kept in the cache
//! directory) and enforced inside WebKit's networking, before a request is
//! made. That costs nothing at run time, unlike a JavaScript blocker.

use serde_json::{Value, json};

pub const IDENTIFIER: &str = "wisp-shield";

/// Third parties whose only job is to watch or to sell. First-party requests
/// are untouched: a site's own scripts are the site.
const UNWANTED: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "googletagservices.com",
    "google-analytics.com",
    "googletagmanager.com",
    "adservice.google.com",
    "amazon-adsystem.com",
    "adnxs.com",
    "adsrvr.org",
    "criteo.com",
    "criteo.net",
    "taboola.com",
    "outbrain.com",
    "rubiconproject.com",
    "pubmatic.com",
    "openx.net",
    "casalemedia.com",
    "smartadserver.com",
    "sharethrough.com",
    "indexww.com",
    "bidswitch.net",
    "33across.com",
    "teads.tv",
    "moatads.com",
    "adroll.com",
    "scorecardresearch.com",
    "quantserve.com",
    "chartbeat.com",
    "hotjar.com",
    "mouseflow.com",
    "fullstory.com",
    "clarity.ms",
    "mixpanel.com",
    "amplitude.com",
    "segment.com",
    "segment.io",
    "branch.io",
    "appsflyer.com",
    "adjust.com",
    "analytics.tiktok.com",
    "connect.facebook.net",
    "ads-twitter.com",
    "analytics.twitter.com",
];

/// The few slots that are reliably an advertisement and nothing else. Kept
/// short on purpose: a generous cosmetic list is how a blocker starts eating
/// the page it was meant to clean.
const SLOTS: &[&str] = &[
    ".adsbygoogle",
    "ins.adsbygoogle",
    "[id^=\"google_ads_\"]",
    "[id^=\"div-gpt-ad\"]",
    "[id^=\"taboola-\"]",
    "#taboola-below-article",
    "iframe[src*=\"doubleclick.net\"]",
    "iframe[src*=\"googlesyndication\"]",
    "iframe[src*=\"amazon-adsystem\"]",
    ".ad-slot",
    ".ad-slot-container",
    ".top-banner-ad-container",
    ".ad-leaderboard",
    ".ad-billboard",
    ".ad-giga",
    ".ad-mpu",
    ".ad-mrec",
    ".ad-unit",
    ".adunit",
    ".adslot",
    ".dfp-ad",
    ".gpt-ad",
    ".w_ad",
];

/// Slots with names too plain to hide everywhere, hidden only on the sites
/// EasyList's own site rules hide them on.
const SLOTS_BY_SITE: &[(&[&str], &str)] = &[
    (&["*as.com", "*elpais.com"], ".ad"),
    (&["*theguardian.com"], ".top-fronts-banner-ad-container"),
    (&["*independent.co.uk", "*the-independent.com"], "#billboard-wrapper"),
    (&["*cnn.com"], ".ad-slot-header__wrapper"),
];

/// The content-blocker patterns for a third-party request to this
/// registrable domain, on the web or a socket. WebKit rejects `|`, so the
/// schemes and the two ways a host can end are written as separate
/// patterns. The domain ends at the host boundary, so `adjust.com` does not
/// match `adjust.comcast.net`.
pub fn blocker(domain: &str) -> Vec<String> {
    let escaped = domain.replace('.', "\\.");
    let mut patterns = Vec::with_capacity(4);
    for scheme in ["https?", "wss?"] {
        patterns.push(format!("^{scheme}://([^/?#]+\\.)?{escaped}[:/?#]"));
        patterns.push(format!("^{scheme}://([^/?#]+\\.)?{escaped}$"));
    }
    patterns
}

/// Whether `url`'s host is `domain` or a subdomain of it.
#[allow(dead_code)] // The compiled rule is what WebKit runs. Tests call this form of the same decision.
pub fn blocks_host(domain: &str, url: &str) -> bool {
    let Some(rest) = url.split_once("://") else { return false };
    let scheme = &url[..url.len() - rest.1.len() - 3];
    if !matches!(scheme, "http" | "https" | "ws" | "wss") {
        return false;
    }
    let host = rest.1.split(['/', '?', '#', ':']).next().unwrap_or("");
    let host = host.trim_end_matches('.');
    host == domain || host.ends_with(&format!(".{domain}"))
}

pub fn rules() -> String {
    let mut rules: Vec<Value> = UNWANTED
        .iter()
        .flat_map(|domain| {
            blocker(domain).into_iter().map(|pattern| {
                json!({
                    "trigger": { "url-filter": pattern, "load-type": ["third-party"] },
                    "action": { "type": "block" }
                })
            })
        })
        .collect();
    rules.push(json!({
        "trigger": { "url-filter": ".*" },
        "action": { "type": "css-display-none", "selector": SLOTS.join(", ") }
    }));
    for (sites, selector) in SLOTS_BY_SITE {
        rules.push(json!({
            "trigger": { "url-filter": ".*", "if-domain": sites },
            "action": { "type": "css-display-none", "selector": selector }
        }));
    }
    Value::Array(rules).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::glib;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn save(dir: &std::path::Path, identifier: &str, json: &str) -> Result<(), String> {
        let filters = webkit6::UserContentFilterStore::new(&dir.to_string_lossy());
        let source = glib::Bytes::from_owned(json.as_bytes().to_vec());
        let done: Rc<RefCell<Option<Result<(), String>>>> = Rc::default();
        let out = done.clone();
        let loop_ = glib::MainLoop::new(None, false);
        let quit = loop_.clone();
        filters.save(identifier, &source, None::<&gtk::gio::Cancellable>, move |result| {
            *out.borrow_mut() = Some(result.map(|_| ()).map_err(|err| err.to_string()));
            quit.quit();
        });
        loop_.run();
        done.borrow_mut().take().unwrap()
    }

    #[test]
    fn shield_rules_compile_on_webkit() {
        gtk::init().unwrap();
        let dir = std::env::temp_dir().join(format!("wisp-shield-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let ok = save(&dir, "wisp-shield-test", &rules());
        assert!(ok.is_ok(), "rules() did not compile: {ok:?}");

        let bad = save(&dir, "wisp-shield-test-bad", r#"[{"trigger":{"url-filter":"^(https?|wss?)://adjust\\.com"},"action":{"type":"block"}}]"#);
        assert!(
            bad.as_ref().err().is_some_and(|err| err.contains("Invalid or unsupported regular expression")),
            "the | pattern did not fail with the toast text: {bad:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shield_rules_stop_at_the_host_boundary() {
        let compiled: serde_json::Value = serde_json::from_str(&rules()).unwrap();
        let filters: Vec<&str> =
            compiled.as_array().unwrap().iter().filter_map(|rule| rule["trigger"]["url-filter"].as_str()).collect();
        assert!(filters.iter().all(|filter| !filter.contains('|')));
        assert!(filters.contains(&"^https?://([^/?#]+\\.)?adjust\\.com[:/?#]"));
        assert!(filters.contains(&"^wss?://([^/?#]+\\.)?adjust\\.com[:/?#]"));
        assert!(blocks_host("adjust.com", "https://adjust.com/pixel"));
        assert!(blocks_host("adjust.com", "wss://track.adjust.com/socket"));
        assert!(blocks_host("adjust.com", "ws://adjust.com"));
        assert!(!blocks_host("adjust.com", "https://adjust.comcast.net/"));
        assert!(!blocks_host("adjust.com", "https://notadjust.com/"));
        assert!(!blocks_host("adjust.com", "file://adjust.com/x"));
    }
}
