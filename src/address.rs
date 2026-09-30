//! What you type has to be a place, or it is a search.
//!
//! `url_from` either hands back a URL or hands back nothing. Nothing means
//! "search for these words" to the caller. There is no guessing in between:
//! "hello world" is not a website, and neither is "todo".

use url::Url;

/// Schemes a tab can show itself. Anything else typed with a scheme
/// (mailto:, a custom app link) is somebody else's job.
const OURS: [&str; 5] = ["http", "https", "file", "about", "data"];

pub fn url_from(typed: &str) -> Option<Url> {
    let text = typed.trim();
    if text.is_empty() || text.contains(' ') {
        return None;
    }

    // Written with a scheme, it is taken at its word.
    if let Some(split) = text.find("://") {
        let scheme = text[..split].to_ascii_lowercase();
        if !OURS.contains(&scheme.as_str()) {
            return None;
        }
        return Url::parse(text).ok();
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("about:") || lower.starts_with("data:") {
        return Url::parse(text).ok();
    }

    // Everything else has to look like a host before it gets a scheme.
    let head: &str = text.split(['/', '?', '#']).next().unwrap_or(text);
    if head.contains('@') {
        return None; // an email address
    }
    if head.starts_with("[::1]") {
        return Url::parse(&format!("http://{text}")).ok();
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

fn is_local(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host == "127.0.0.1"
        || host == "0.0.0.0"
        || host.starts_with("192.168.")
        || host.starts_with("10.")
        || private_range(host)
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

    fn go(s: &str) -> Option<String> {
        url_from(s).map(|u| u.to_string())
    }

    #[test]
    fn places() {
        assert_eq!(go("archlinux.org").as_deref(), Some("https://archlinux.org/"));
        assert_eq!(go("wiki.archlinux.org/title/Pacman").as_deref(), Some("https://wiki.archlinux.org/title/Pacman"));
        assert_eq!(go("localhost:3000").as_deref(), Some("http://localhost:3000/"));
        assert_eq!(go("0.0.0.0:8080").as_deref(), Some("http://localhost:8080/"));
        assert_eq!(go("192.168.1.1").as_deref(), Some("http://192.168.1.1/"));
        assert_eq!(go("172.20.0.2").as_deref(), Some("http://172.20.0.2/"));
        assert_eq!(go("printer.local").as_deref(), Some("http://printer.local/"));
        assert_eq!(go("http://example.com").as_deref(), Some("http://example.com/"));
        assert_eq!(go("about:blank").as_deref(), Some("about:blank"));
    }

    #[test]
    fn searches() {
        assert_eq!(go("hello world"), None);
        assert_eq!(go("todo"), None);
        assert_eq!(go("1.2"), None);
        assert_eq!(go("me@example.com"), None);
        assert_eq!(go("mailto://x"), None);
        assert_eq!(go(""), None);
    }

    #[test]
    fn editing() {
        assert_eq!(editable("https://archlinux.org/"), "archlinux.org");
        assert_eq!(editable("https://archlinux.org/news/"), "archlinux.org/news/");
        assert_eq!(editable("http://example.com/"), "http://example.com/");
        assert_eq!(editable("http://localhost:3000/"), "localhost:3000");
    }

    #[test]
    fn prettiness() {
        assert_eq!(pretty("https://www.archlinux.org/"), "archlinux.org");
        assert_eq!(pretty("https://archlinux.org/news"), "archlinux.org/news");
    }
}
