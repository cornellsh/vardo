//! Does this login belong to this page?
//!
//! The most security-sensitive decision the browser extension depends on, so it
//! lives here, in Rust, where every rule has a test, rather than in JavaScript
//! running next to the page it is judging.
//!
//! The page address always comes from the browser's own tab information, never
//! from anything the page says about itself. The login's address comes from
//! the vault, where people type it by hand, so it is parsed forgivingly: a bare
//! `github.com` is the common case, not a mistake.
//!
//! # The rules
//!
//! - Only `http` and `https` pages, and only logins whose address is one of
//!   those or has no scheme at all.
//! - **No downgrade.** A login saved for `https` never matches an `http` page,
//!   and a login with no scheme is treated as `https`. Only a login explicitly
//!   saved as `http://` matches an insecure page.
//! - **Same port.** A different port is a different service.
//! - **Same host,** ignoring a leading `www.`, is [`Match::Exact`]. These are
//!   what the keyboard shortcut fills.
//! - **A subdomain of the saved host** is [`Match::Subdomain`]. These are only
//!   offered for an explicit choice: filling them automatically turns
//!   cross-site scripting on any subdomain into theft of the main site's
//!   login, which is how ten of eleven managers were attacked in 2025.
//! - **Never a public suffix.** A login saved for `github.io` or `co.uk` is not
//!   one site, and does not match every site beneath it.

use url::{Host, Url};

/// How well a login's address fits a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Match {
    /// The same site. Safe to fill from the keyboard shortcut.
    Exact,
    /// A subdomain of the saved site. Shown, but only filled when chosen.
    Subdomain,
}

/// Compares a login's saved address with the page's.
///
/// `None` means the login must not be offered for this page at all.
pub fn classify(saved: &str, page: &str) -> Option<Match> {
    let page = parse_page(page)?;
    let saved = parse_saved(saved)?;

    // No downgrade. Upgrading an `http` login to an `https` page is fine.
    if saved.secure && !page.secure {
        return None;
    }

    // A login with no explicit port belongs to the default port, which is why
    // `http://host` still matches `https://host` despite 80 and 443.
    if saved.port != page.port {
        return None;
    }

    match (&saved.host, &page.host) {
        (HostKind::Ip(a), HostKind::Ip(b)) => (a == b).then_some(Match::Exact),
        (HostKind::Name(saved), HostKind::Name(page)) => {
            let saved = strip_www(saved);
            let page = strip_www(page);
            if saved == page {
                Some(Match::Exact)
            } else if is_subdomain_of(page, saved) && !is_public_suffix(saved) {
                Some(Match::Subdomain)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// True when the saved address names a path that this page sits under.
///
/// Used only to order logins that already match, so that a login saved for
/// `example.com/admin` comes first on the admin pages.
pub fn path_fits(saved: &str, page: &str) -> bool {
    let (Some(saved), Some(page)) = (url_of_saved(saved), Url::parse(page).ok()) else {
        return false;
    };
    let path = saved.path().trim_end_matches('/');
    !path.is_empty() && page.path().starts_with(path)
}

struct Parsed {
    secure: bool,
    host: HostKind,
    /// `None` for the scheme's default port.
    port: Option<u16>,
}

enum HostKind {
    Name(String),
    Ip(std::net::IpAddr),
}

fn parse_page(page: &str) -> Option<Parsed> {
    let url = Url::parse(page).ok()?;
    let secure = match url.scheme() {
        "https" => true,
        "http" => false,
        _ => return None,
    };
    parsed(&url, secure)
}

fn parse_saved(saved: &str) -> Option<Parsed> {
    let url = url_of_saved(saved)?;
    let secure = url.scheme() == "https";
    parsed(&url, secure)
}

/// The saved address as a URL, adding `https://` when it has no scheme.
fn url_of_saved(saved: &str) -> Option<Url> {
    let saved = saved.trim();
    if saved.is_empty() {
        return None;
    }
    let has_scheme = saved.contains("://");
    let url = if has_scheme {
        Url::parse(saved).ok()?
    } else {
        Url::parse(&format!("https://{saved}")).ok()?
    };
    matches!(url.scheme(), "http" | "https").then_some(url)
}

fn parsed(url: &Url, secure: bool) -> Option<Parsed> {
    // `port()` is already `None` for the scheme's default, so `:443` on https
    // and no port at all compare equal.
    let port = url.port();
    let host = match url.host()? {
        Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_string();
            if name.is_empty() {
                return None;
            }
            HostKind::Name(name)
        }
        Host::Ipv4(ip) => HostKind::Ip(ip.into()),
        Host::Ipv6(ip) => HostKind::Ip(ip.into()),
    };
    Some(Parsed { secure, host, port })
}

fn strip_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

fn is_subdomain_of(page: &str, saved: &str) -> bool {
    page.len() > saved.len()
        && page.ends_with(saved)
        && page.as_bytes()[page.len() - saved.len() - 1] == b'.'
}

/// True for `com`, `co.uk`, `github.io` and anything else on the public suffix
/// list, where different names belong to different owners.
fn is_public_suffix(host: &str) -> bool {
    psl::suffix_str(host).is_some_and(|suffix| suffix == host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(saved: &str, page: &str) -> Option<Match> {
        classify(saved, page)
    }

    #[test]
    fn the_same_site_is_exact_whatever_the_path() {
        assert_eq!(
            m("https://github.com", "https://github.com/login"),
            Some(Match::Exact)
        );
        assert_eq!(
            m("https://github.com/login", "https://github.com/session"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn a_bare_address_is_the_common_case_not_a_mistake() {
        assert_eq!(
            m("github.com", "https://github.com/login"),
            Some(Match::Exact)
        );
        assert_eq!(
            m("  github.com/  ", "https://github.com/"),
            Some(Match::Exact)
        );
        assert_eq!(
            m("github.com/login", "https://github.com/login"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn hosts_compare_without_regard_to_case_or_a_trailing_dot() {
        assert_eq!(
            m("https://GitHub.COM", "https://github.com/"),
            Some(Match::Exact)
        );
        assert_eq!(
            m("github.com", "https://github.com./login"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn www_is_the_same_site() {
        assert_eq!(
            m("https://www.amazon.com", "https://amazon.com/"),
            Some(Match::Exact)
        );
        assert_eq!(
            m("amazon.com", "https://www.amazon.com/ap/signin"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn a_subdomain_matches_but_never_as_exact() {
        // What the shortcut must not fill without a choice: a script on any
        // subdomain could otherwise collect the main site's login.
        assert_eq!(
            m("google.com", "https://accounts.google.com/"),
            Some(Match::Subdomain)
        );
        assert_eq!(
            m("example.com", "https://a.b.example.com/"),
            Some(Match::Subdomain)
        );
    }

    #[test]
    fn a_parent_or_a_sibling_is_not_a_match() {
        assert_eq!(m("accounts.google.com", "https://google.com/"), None);
        assert_eq!(m("accounts.google.com", "https://mail.google.com/"), None);
    }

    #[test]
    fn a_lookalike_suffix_is_not_a_subdomain() {
        // `ends_with` alone would accept these.
        assert_eq!(m("github.com", "https://evilgithub.com/"), None);
        assert_eq!(m("github.com", "https://github.com.evil.com/"), None);
    }

    #[test]
    fn credentials_in_the_address_do_not_decide_the_host() {
        // The host here is evil.com. A matcher that splits on "/" and takes the
        // first piece sees github.com, and fills a GitHub password into it.
        assert_eq!(m("github.com", "https://github.com@evil.com/login"), None);
        assert_eq!(
            m("github.com", "https://user:pw@github.com/"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn a_public_suffix_is_not_one_site() {
        // Everyone gets their own name under github.io; a login saved for the
        // suffix itself must not follow the user to every one of them.
        assert_eq!(m("github.io", "https://attacker.github.io/"), None);
        assert_eq!(m("co.uk", "https://bank.co.uk/"), None);
        assert_eq!(m("com", "https://anything.com/"), None);
        // A name registered under one is a site like any other.
        assert_eq!(
            m("alice.github.io", "https://alice.github.io/"),
            Some(Match::Exact)
        );
        assert_eq!(
            m("bank.co.uk", "https://online.bank.co.uk/"),
            Some(Match::Subdomain)
        );
    }

    #[test]
    fn https_never_downgrades_to_http() {
        assert_eq!(m("https://example.com", "http://example.com/"), None);
        // No scheme means https: an insecure page needs an explicit http login.
        assert_eq!(m("example.com", "http://example.com/"), None);
        assert_eq!(
            m("http://192.168.1.1", "http://192.168.1.1/"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn http_upgrades_to_https() {
        assert_eq!(
            m("http://example.com", "https://example.com/"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn a_different_port_is_a_different_service() {
        assert_eq!(m("https://example.com", "https://example.com:8443/"), None);
        assert_eq!(m("https://example.com:8443", "https://example.com/"), None);
        assert_eq!(
            m("https://example.com:8443", "https://example.com:8443/x"),
            Some(Match::Exact)
        );
        // The default port written out is still the default port.
        assert_eq!(
            m("https://example.com:443", "https://example.com/"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn addresses_match_only_exactly() {
        assert_eq!(m("10.0.0.1", "https://10.0.0.1/"), Some(Match::Exact));
        assert_eq!(m("10.0.0.1", "https://10.0.0.2/"), None);
        assert_eq!(
            m("https://[::1]:8080", "https://[::1]:8080/"),
            Some(Match::Exact)
        );
        assert_eq!(m("example.com", "https://93.184.216.34/"), None);
    }

    #[test]
    fn international_names_compare_in_one_form() {
        // Chrome reports the punycode form; people type the Unicode one.
        assert_eq!(
            m("bücher.de", "https://xn--bcher-kva.de/"),
            Some(Match::Exact)
        );
    }

    #[test]
    fn only_web_pages_and_web_logins_take_part() {
        assert_eq!(m("github.com", "chrome://settings/passwords"), None);
        assert_eq!(m("github.com", "file:///home/user/login.html"), None);
        assert_eq!(m("ssh://github.com", "https://github.com/"), None);
        assert_eq!(
            m("androidapp://com.github.android", "https://github.com/"),
            None
        );
        assert_eq!(m("", "https://github.com/"), None);
        assert_eq!(m("github.com", "not a url"), None);
    }

    #[test]
    fn a_path_orders_matches_but_never_creates_one() {
        assert!(path_fits(
            "example.com/admin",
            "https://example.com/admin/users"
        ));
        assert!(!path_fits("example.com/admin", "https://example.com/shop"));
        assert!(!path_fits("example.com", "https://example.com/admin"));
        assert_eq!(
            m("example.com/admin", "https://example.com/shop"),
            Some(Match::Exact)
        );
    }
}
