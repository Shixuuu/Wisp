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

pub fn rules() -> String {
    let mut rules: Vec<Value> = UNWANTED
        .iter()
        .map(|domain| {
            let escaped = domain.replace('.', "\\.");
            json!({
                "trigger": { "url-filter": format!("^https?://([^/]+\\.)?{escaped}"), "load-type": ["third-party"] },
                "action": { "type": "block" }
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
    #[test]
    fn rules_are_json() {
        let v: serde_json::Value = serde_json::from_str(&super::rules()).unwrap();
        let list = v.as_array().unwrap();
        assert_eq!(list.len(), super::UNWANTED.len() + 1 + super::SLOTS_BY_SITE.len());
        assert_eq!(list[0]["trigger"]["url-filter"], "^https?://([^/]+\\.)?doubleclick\\.net");
    }
}
