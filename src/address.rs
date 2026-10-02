//! What you type has to be a place, or it is a search.
//!
//! `url_from` either hands back a URL or hands back nothing. Nothing means
//! "search for these words" to the caller. There is no guessing in between:
//! "hello world" is not a website, and neither is "todo".

use url::Url;

/// Schemes a tab can show itself. Anything else typed with a scheme
/// (mailto:, a custom app link) is somebody else's job.
const OURS: [&str; 5] = ["http", "https", "file", "about", "data"];

/// A scheme token at the start of `text`, before `://`.
/// `example.com/r?u=https://x.com` has none: the `://` is not at the front.
pub fn leading_scheme(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.first().is_none_or(|c| !c.is_ascii_alphabetic()) {
        return None;
    }
    let mut end = 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'+' | b'.' | b'-')) {
        end += 1;
    }
    text[end..].starts_with("://").then_some(&text[..end])
}

pub fn url_from(typed: &str) -> Option<Url> {
    let text = typed.trim();
    if text.is_empty() || text.contains(' ') {
        return None;
    }

    // A scheme counts only when it is a token at the very start.
    if let Some(scheme) = leading_scheme(text) {
        let scheme = scheme.to_ascii_lowercase();
        if !OURS.contains(&scheme.as_str()) {
            return None;
        }
        return Url::parse(text).ok().map(reachable);
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("about:") || lower.starts_with("data:") {
        return Url::parse(text).ok();
    }

    // Everything else has to look like a host before it gets a scheme.
    let head: &str = lower.split(['/', '?', '#']).next().unwrap_or(&lower);
    if head.contains('@') {
        return None; // an email address
    }
    if head.starts_with("[::1]") {
        return Url::parse(&format!("http://{text}")).ok().map(reachable);
    }
    let host = head.split(':').next().unwrap_or(head);
    if !looks_like_host(host) {
        return None;
    }

    // A local server almost never has a certificate, so https there is a
    // connection failure rather than a page.
    let scheme = if is_local(host) { "http" } else { "https" };
    let url = Url::parse(&format!("{scheme}://{text}")).ok()?;
    Some(reachable(url))
}

/// A scheme token at the start, with or without `//`. `javascript:alert(1)` counts.
fn scheme_prefix(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.first().is_none_or(|c| !c.is_ascii_alphabetic()) {
        return None;
    }
    let mut end = 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'+' | b'.' | b'-')) {
        end += 1;
    }
    text[end..].starts_with(':').then_some(&text[..end])
}

/// What a command-line argument opens. A scheme Wisp does not show becomes a
/// search, so `javascript:` is never handed to the page as a raw address.
/// An existing local file wins only when the argument has no scheme.
pub fn cli_target(arg: &str, file_exists: bool, file_uri: &str, search: impl Fn(&str) -> String) -> String {
    if file_exists && scheme_prefix(arg).is_none() {
        return file_uri.to_string();
    }
    match url_from(arg) {
        Some(url) => url.to_string(),
        None => search(arg),
    }
}

fn is_local(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || ipv4(host)
            && (host == "127.0.0.1"
                || host == "0.0.0.0"
                || host.starts_with("10.")
                || host.starts_with("192.168.")
                || private_range(host))
}

fn ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|part| !part.is_empty() && part.parse::<u8>().is_ok())
}

/// 172.16.0.0 to 172.31.255.255, where Docker and many offices put machines.
fn private_range(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts[0] == "172" && matches!(parts[1].parse::<u8>(), Ok(16..=31))
}

/// Dev servers print 0.0.0.0 as the address to open. WebKit refuses to go
/// there, so it is opened as localhost: the same server.
pub fn reachable(mut url: Url) -> Url {
    if url.host_str() == Some("0.0.0.0") && matches!(url.scheme(), "http" | "https") {
        let _ = url.set_host(Some("localhost"));
    }
    url
}

fn looks_like_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    let labels: Vec<&str> = host.split('.').collect();
    // Four numbers is an address on the local network as often as not.
    if labels.len() == 4 && labels.iter().all(|l| l.parse::<u8>().is_ok()) {
        return true;
    }
    if labels.len() < 2 {
        return false;
    }
    let good = labels.iter().all(|l| {
        !l.is_empty() && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_alphanumeric() || c == '-')
    });
    if !good {
        return false;
    }
    // A dotted thing ending in letters is a domain; ending in digits it is a
    // version number.
    let tld = labels[labels.len() - 1];
    tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic)
}

/// A page's address as the field shows it for editing: the scheme is left off
/// only when typing the result would put that same scheme back.
pub fn editable(page: &str) -> String {
    let Ok(url) = Url::parse(page) else { return page.to_string() };
    let full = url.as_str();
    let prefix = format!("{}://", url.scheme());
    let Some(short) = full.strip_prefix(&prefix) else { return full.to_string() };
    let mut candidates = vec![short.to_string()];
    if url.path() == "/" && url.query().is_none() && url.fragment().is_none() && short.ends_with('/') {
        candidates.insert(0, short[..short.len() - 1].to_string());
    }
    for candidate in candidates {
        if let Some(back) = url_from(&candidate)
            && back.as_str() == full
        {
            return candidate;
        }
    }
    full.to_string()
}

/// Camera, microphone, location and notification answers are per origin:
/// scheme, host and port. Two host-less pages do not share one answer.
pub fn permission_key(address: &str, kind: &str) -> String {
    match Url::parse(address) {
        Ok(url) => {
            let host = url.host_str().unwrap_or("");
            let port = url.port_or_known_default().unwrap_or(0);
            format!("{}://{host}:{port} {kind}", url.scheme())
        }
        Err(_) => format!("unknown {kind}"),
    }
}

/// A host without a leading `www.`.
pub fn bare_host(url: &str) -> Option<String> {
    let host = Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    Some(host.strip_prefix("www.").map(str::to_string).unwrap_or(host))
}

/// What a tab says before the page has told us its title.
pub fn pretty(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else { return url.to_string() };
    let Some(host) = bare_host(url) else { return url.to_string() };
    let path = parsed.path();
    if path.is_empty() || path == "/" { host } else { format!("{host}{path}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scheme_counts_only_when_it_leads() {
        assert_eq!(leading_scheme("https://example.com"), Some("https"));
        assert_eq!(leading_scheme("example.com/r?u=https://x.com"), None);
        assert!(url_from("javascript:alert(1)").is_none());
        assert!(url_from("see https://example.com").is_none());

        let local = url_from("LOCALHOST:3000").unwrap();
        assert_eq!(local.scheme(), "http");
        assert_eq!(local.host_str(), Some("localhost"));
        assert_eq!(local.port(), Some(3000));

        let rewritten = url_from("http://0.0.0.0:8080/app").unwrap();
        assert_eq!(rewritten.host_str(), Some("localhost"));
        assert_eq!(rewritten.port(), Some(8080));
        assert!(rewritten.path().contains("app"));

        assert_eq!(url_from("10.fm").unwrap().scheme(), "https");
        assert_eq!(url_from("192.168.example.com").unwrap().scheme(), "https");
        assert_eq!(url_from("10.0.0.8").unwrap().scheme(), "http");

        let script = cli_target("javascript:alert(1)", true, "file:///tmp/x", |q| format!("search:{q}"));
        assert!(script.starts_with("search:"));
        assert!(!script.starts_with("javascript:"));
        assert_eq!(
            cli_target("notes.txt", true, "file:///tmp/notes.txt", |_| "search".into()),
            "file:///tmp/notes.txt"
        );
        assert_eq!(
            cli_target("https://example.com", true, "file:///tmp/x", |_| "search".into()),
            "https://example.com/"
        );
    }

    #[test]
    fn permissions_distinguish_scheme_and_port() {
        assert_eq!(permission_key("https://example.com/a", "camera"), "https://example.com:443 camera");
        assert_ne!(
            permission_key("http://example.com:8080/", "camera"),
            permission_key("https://example.com/", "camera")
        );
        assert_eq!(permission_key("not a url", "geo"), "unknown geo");
    }
}
