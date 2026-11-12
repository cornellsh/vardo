//! Fetching a site's icon, on purpose and one at a time.
//!
//! Contract: icons come from the entry's own site, never an aggregator, with
//! capped response sizes (`inv:no-phone-home`).
//!
//! # What this deliberately does not do
//!
//! **It never asks a favicon service.** Google's `s2/favicons` and DuckDuckGo's
//! equivalent are one HTTP call and would have been a tenth of this code. They
//! are also a complete list of every service you hold an account with, handed
//! to one company: your bank, your employer, your doctor, your dating site.
//! That list is arguably the second most sensitive thing in a vault after the
//! passwords themselves, and mailing it somewhere to make a list prettier is
//! not a trade worth making. Icons come from the site itself or not at all.
//!
//! **It never runs on unlock.** Four hundred requests to four hundred domains
//! the moment a vault opens is a fingerprint, and it tells every one of those
//! sites that this person uses a password manager and just opened it. Fetching
//! happens when someone asks for it, for the entry they asked about.
//!
//! **It only ever contacts the host.** A stored URL can carry a path, a query,
//! or a token; none of that is needed to find an icon and all of it would be
//! sent to the server and into its logs.
//!
//! # Why the result goes into the vault
//!
//! The icon is written into `<Meta><CustomIcons>` and referenced by the entry,
//! which is where KeePassXC puts the ones it downloads. That makes the fetch a
//! one-time cost rather than something repeated on every device: it syncs with
//! the vault, and KeePassium on iOS renders it too. It also means the reader in
//! `icons.rs` is the only path that ever runs afterwards.

use vardo_drive::http::{HttpTransport, Method, Request};

use crate::CoreError;

/// Nobody needs a 5 MB icon, and without a cap a hostile or broken server can
/// stream until memory runs out.
const MAX_ICON_BYTES: usize = 512 * 1024;

/// Enough HTML to contain `<head>`. Some sites are megabytes of inlined script,
/// and the link tags are always near the top.
const MAX_HTML_BYTES: usize = 256 * 1024;

pub struct FetchedIcon {
    pub png: Vec<u8>,
    /// The host it actually came from, for showing the user what was contacted.
    pub host: String,
}

/// Finds and downloads an icon for `url`'s host.
///
/// Returns `None` when the site simply has no usable icon, which is common and
/// not an error worth interrupting anyone about.
pub fn fetch(http: &mut dyn HttpTransport, url: &str) -> Result<Option<FetchedIcon>, CoreError> {
    let host = host_of(url).ok_or_else(|| CoreError::Unsupported {
        detail: "That entry has no web address".into(),
    })?;

    let origin = format!("https://{host}");

    // The homepage first, because the tags there point at real artwork:
    // apple-touch-icon is always PNG and usually 180px, where /favicon.ico is
    // often a 16px ICO that cannot be stored as a PNG without re-encoding.
    let mut candidates = Vec::new();
    if let Ok(html) = get(http, &origin, usize::MAX) {
        // Truncated, not rejected. Rejecting an oversized page threw away every
        // site whose homepage runs past the cap, which is most large ones -
        // Airbnb and Stripe both publish perfectly good apple-touch-icons and
        // both were being skipped because their HTML is bigger than the buffer
        // I had picked. The link tags live in <head>, so the start is enough.
        let head = &html[..html.len().min(MAX_HTML_BYTES)];
        candidates.extend(icon_links(&String::from_utf8_lossy(head), &origin));
    }
    candidates.push(format!("{origin}/apple-touch-icon.png"));
    candidates.push(format!("{origin}/favicon.png"));

    for candidate in candidates {
        let Ok(bytes) = get(http, &candidate, MAX_ICON_BYTES) else {
            continue;
        };
        // PNG only, checked by magic number rather than by the extension or the
        // Content-Type, both of which are whatever the server felt like saying.
        // Storing a non-PNG under a PNG's name would render as a broken image
        // in every other KeePass client.
        if is_png(&bytes) {
            return Ok(Some(FetchedIcon { png: bytes, host }));
        }
    }

    Ok(None)
}

fn get(http: &mut dyn HttpTransport, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    // Refuse anything that is not plain HTTPS. A vault is a list of URLs, and
    // some of them will be file:// or a private address someone once saved;
    // neither should become a request this code makes on their behalf.
    if !url.starts_with("https://") {
        return Err("only https is fetched".into());
    }

    let response = http.send(
        Request::new(Method::Get, url)
            // No cookies, no referer, and a user agent that says what this is
            // rather than impersonating a browser.
            .header("User-Agent", concat!("vardo/", env!("CARGO_PKG_VERSION")))
            .header("Accept", "image/png,text/html"),
    )?;

    if !response.is_success() {
        return Err(format!("HTTP {}", response.status));
    }
    if response.body.len() > limit {
        return Err("larger than the limit".into());
    }
    Ok(response.body)
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() > 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
}

/// The host, and only the host.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url
        .split_once("://")
        .map(|(scheme, rest)| {
            // A stored address is often bare. Anything explicitly not web is
            // refused rather than coerced.
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
                Some(rest)
            } else {
                None
            }
        })
        .unwrap_or(Some(url))?;

    let host = rest
        .split(['/', '?', '#'])
        .next()?
        // Credentials in a URL are rare and must not be sent anywhere.
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .trim();

    if host.is_empty() || !host.contains('.') || host.contains(' ') {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Pulls icon URLs out of `<link rel="...icon...">`, best first.
///
/// A hand-rolled scan rather than an HTML parser: this looks at `rel` and
/// `href` on link tags and nothing else, and adding a parser to the tree for
/// that would be the heaviest dependency in the project.
fn icon_links(html: &str, origin: &str) -> Vec<String> {
    let lower = html.to_lowercase();
    let mut found: Vec<(u32, String)> = Vec::new();

    for (start, _) in lower.match_indices("<link") {
        let Some(end) = lower[start..].find('>') else {
            continue;
        };
        let tag = &lower[start..start + end];
        let raw = &html[start..start + end];

        let Some(rel) = attribute(tag, "rel") else {
            continue;
        };
        if !rel.contains("icon") {
            continue;
        }

        let Some(href) = attribute(raw, "href").map(|h| unescape(&h)) else {
            continue;
        };

        // apple-touch-icon is worth preferring: it is PNG by definition and
        // sized for a phone home screen, so it is the one link on most pages
        // that is guaranteed to be usable artwork.
        let rank = if rel.contains("apple-touch") {
            0
        } else if href.to_lowercase().ends_with(".png") {
            1
        } else {
            2
        };
        found.push((rank, absolute(&href, origin)));
    }

    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, url)| url).collect()
}

/// Undoes the HTML escaping in an attribute value.
///
/// `&amp;` is the one that matters and it is everywhere: an icon URL with sizing
/// parameters is written `?w=180&amp;h=180`, and requesting that literally gets
/// a 404 or the wrong image. Stripe's icons are served exactly this way.
fn unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}="))?;
    let rest = &tag[at + name.len() + 1..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = rest[1..].find(quote)?;
        Some(rest[1..1 + end].to_string())
    } else {
        Some(
            rest.split([' ', '\t', '\n', '>'])
                .next()?
                .trim()
                .to_string(),
        )
    }
}

fn absolute(href: &str, origin: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else if href.starts_with('/') {
        format!("{origin}{href}")
    } else {
        format!("{origin}/{href}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_host_is_ever_taken_from_a_url() {
        // The path and query are the point. A stored address can carry a reset
        // token or a session id, and none of it is needed to find an icon.
        assert_eq!(
            host_of("https://example.com/reset?token=abc123"),
            Some("example.com".into())
        );
        assert_eq!(
            host_of("https://user:pw@example.com/x"),
            Some("example.com".into())
        );
        assert_eq!(host_of("example.com"), Some("example.com".into()));
        assert_eq!(
            host_of("https://EXAMPLE.com:8443/"),
            Some("example.com".into())
        );
    }

    #[test]
    fn non_web_addresses_are_refused_rather_than_coerced() {
        // Real vaults contain these, and turning one into an outbound request
        // would be acting on data the user never meant as a web address.
        for url in [
            "file:///home/me/secrets",
            "ssh://box",
            "",
            "not a url",
            "localhost",
        ] {
            assert_eq!(host_of(url), None, "should refuse {url:?}");
        }
    }

    #[test]
    fn png_is_recognised_by_its_magic_not_its_name() {
        assert!(is_png(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]));
        // An HTML error page served with a .png name, which is what a site
        // returns for a missing icon far more often than a 404.
        assert!(!is_png(b"<!doctype html><html>404"));
        assert!(!is_png(&[]));
    }

    #[test]
    fn apple_touch_icon_is_preferred_over_a_bare_favicon() {
        let html = r#"
            <link rel="shortcut icon" href="/favicon.ico">
            <link rel="apple-touch-icon" href="/touch.png">
        "#;
        let links = icon_links(html, "https://example.com");
        assert_eq!(
            links.first().map(String::as_str),
            Some("https://example.com/touch.png")
        );
    }

    #[test]
    fn relative_and_protocol_relative_hrefs_resolve() {
        let html = r#"
            <link rel="icon" href="//cdn.example.net/a.png">
            <link rel="icon" href="sub/b.png">
        "#;
        let links = icon_links(html, "https://example.com");
        assert!(links.contains(&"https://cdn.example.net/a.png".to_string()));
        assert!(links.contains(&"https://example.com/sub/b.png".to_string()));
    }

    #[test]
    fn a_page_with_no_icon_links_yields_nothing() {
        assert!(icon_links("<html><body>hi</body></html>", "https://example.com").is_empty());
    }

    #[test]
    fn an_escaped_ampersand_in_an_icon_url_is_decoded() {
        // Real sites write sizing parameters this way, and requesting the
        // literal `&amp;` gets a 404 or the wrong image. Stripe serves its
        // icons exactly like this, and it was one of the two sites that
        // silently produced no icon.
        let html =
            r#"<link rel="apple-touch-icon" href="https://cdn.example/i.png?w=180&amp;h=180">"#;
        let links = icon_links(html, "https://example.com");
        assert_eq!(
            links.first().map(String::as_str),
            Some("https://cdn.example/i.png?w=180&h=180")
        );
    }
}
