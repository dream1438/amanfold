//! Which saved logins belong to which web page.
//!
//! Autofill is only as safe as this matching. Every app (browser extension,
//! Android autofill, iOS AutoFill) calls these functions, so a page gets the
//! same answer everywhere:
//!
//! * A login saved for `https://accounts.example.com` matches pages on the
//!   same **site** (registrable domain, from the Public Suffix List), so
//!   `www.example.com` and `login.example.com` match, but `example.com.evil.io`,
//!   `example-login.com` and `user.github.io` vs `other.github.io` do not.
//! * A login saved over HTTPS never matches an HTTP page (a network attacker
//!   could serve that page). One saved over HTTP matches both.
//! * IP addresses, `localhost`, and hosts under an unknown suffix (`nas.lan`)
//!   match only the same host and port.
//! * Only `http` and `https` pages ever match. `file:`, `data:`,
//!   `javascript:`, `blob:` and extension pages never do.
//! * Hosts are compared in their ASCII (punycode) form, so `pаypal.com` with a
//!   Cyrillic `а` is a different site from `paypal.com`.
//!
//! [`is_lookalike`] adds a warning for pages that resemble a saved site
//! without being it (`paypa1.com`, `paypal-secure.com`, `pаypal.com`).

use url::{Host, Url};

/// How well a saved URL matches a page. Ordered from no match to best.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SiteMatch {
    /// Different site: never offer this login here.
    None,
    /// Same registrable domain (`login.example.com` for `example.com`).
    Site,
    /// Same host, different scheme or port.
    Host,
    /// Same scheme, host and port.
    Origin,
}

/// A parsed web address, reduced to what matching needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteInfo {
    /// `true` for `https`.
    pub secure: bool,
    /// Host in ASCII form (punycode for international names), lowercase.
    pub host: String,
    /// Host as people read it (Unicode). Show [`SiteInfo::host`] next to it
    /// when the two differ, so look-alike letters are visible.
    pub display_host: String,
    /// Port, with the scheme's default filled in.
    pub port: u16,
    /// Registrable domain (`example.co.uk`), or the whole host when there is
    /// none (IP address, `localhost`, unknown or private suffix).
    pub site: String,
    /// `true` when only the same host and port may match (no known site).
    pub host_only: bool,
}

fn host_only_site(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost")
}

fn info(u: &Url) -> Option<SiteInfo> {
    let secure = match u.scheme() {
        "https" => true,
        "http" => false,
        _ => return None,
    };
    let port = u.port_or_known_default()?;
    let (host, is_ip) = match u.host()? {
        Host::Domain(d) => (d.trim_end_matches('.').to_ascii_lowercase(), false),
        Host::Ipv4(a) => (a.to_string(), true),
        Host::Ipv6(a) => (format!("[{a}]"), true),
    };
    if host.is_empty() {
        return None;
    }
    let registrable = if is_ip || host_only_site(&host) {
        None
    } else {
        psl::domain(host.as_bytes())
            .filter(|d| d.suffix().is_known())
            .and_then(|d| std::str::from_utf8(d.as_bytes()).ok())
            .map(str::to_owned)
    };
    let display_host = if is_ip {
        host.clone()
    } else {
        let (uni, res) = idna::domain_to_unicode(&host);
        if res.is_ok() { uni } else { host.clone() }
    };
    Some(SiteInfo {
        secure,
        host_only: registrable.is_none(),
        site: registrable.unwrap_or_else(|| host.clone()),
        host,
        display_host,
        port,
    })
}

/// Parse a page address. `None` for anything that is not an `http` or
/// `https` URL with a host.
pub fn page_site(page_url: &str) -> Option<SiteInfo> {
    Url::parse(page_url.trim()).ok().as_ref().and_then(info)
}

/// Parse an address saved in a login. People often save `example.com`
/// without a scheme; that is read as `https://example.com`.
pub fn saved_site(saved_url: &str) -> Option<SiteInfo> {
    let s = saved_url.trim();
    if s.is_empty() {
        return None;
    }
    let parsed = if s.contains("://") {
        Url::parse(s)
    } else {
        Url::parse(&format!("https://{s}"))
    };
    parsed.ok().as_ref().and_then(info)
}

/// How a login saved for `saved_url` matches the page at `page_url`.
pub fn match_url(saved_url: &str, page_url: &str) -> SiteMatch {
    match (saved_site(saved_url), page_site(page_url)) {
        (Some(s), Some(p)) => match_sites(&s, &p),
        _ => SiteMatch::None,
    }
}

/// [`match_url`] on already parsed addresses.
pub fn match_sites(saved: &SiteInfo, page: &SiteInfo) -> SiteMatch {
    // Saved over HTTPS: an HTTP page could be served by anyone on the network.
    if saved.secure && !page.secure {
        return SiteMatch::None;
    }
    if saved.host == page.host {
        if saved.port == page.port && saved.secure == page.secure {
            return SiteMatch::Origin;
        }
        if saved.host_only {
            // An IP or local name: another port is another service.
            return if saved.port == page.port {
                SiteMatch::Host
            } else {
                SiteMatch::None
            };
        }
        return SiteMatch::Host;
    }
    if !saved.host_only && !page.host_only && saved.site == page.site {
        return SiteMatch::Site;
    }
    SiteMatch::None
}

// ---------------------------------------------------------------------------
// Look-alikes
// ---------------------------------------------------------------------------

/// The part of a registrable domain people recognise: `paypal` in
/// `paypal.co.uk`, decoded to Unicode.
fn brand(site: &SiteInfo) -> Option<String> {
    if site.host_only {
        return None;
    }
    let suffix = psl::suffix(site.site.as_bytes())?;
    let suffix_len = suffix.as_bytes().len();
    let label = site
        .site
        .get(..site.site.len().checked_sub(suffix_len + 1)?)?;
    let (uni, res) = idna::domain_to_unicode(label);
    res.ok()?;
    Some(uni.to_lowercase())
}

/// Fold characters that look alike, so `pаypa1` and `paypal` compare equal.
/// Covers digits, common Cyrillic and Greek homoglyphs, and letter pairs that
/// read as one letter (`rn` → `m`, `vv` → `w`).
fn skeleton(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let m = match c {
            '0' | 'о' | 'ο' | 'օ' => 'o',
            '1' | 'l' | 'i' | 'і' | 'ι' | 'ӏ' | '|' | 'ı' => 'l',
            '3' | 'е' | 'ε' => 'e',
            '5' | 'ѕ' => 's',
            '4' | 'а' | 'α' => 'a',
            '6' | 'б' => 'b',
            '8' => 'b',
            'р' | 'ρ' => 'p',
            'с' | 'ϲ' => 'c',
            'у' | 'γ' => 'y',
            'х' | 'χ' => 'x',
            'ј' => 'j',
            'ԁ' => 'd',
            'ɡ' => 'g',
            'ν' => 'v',
            'κ' | 'к' => 'k',
            'т' | 'τ' => 't',
            'н' => 'h',
            'м' => 'm',
            'в' => 'b',
            'ω' | 'ш' => 'w',
            '-' | '_' | '.' => continue,
            c => c,
        };
        out.push(m);
    }
    out.replace("rn", "m").replace("vv", "w").replace("cl", "d")
}

fn edit_distance_at_most_one(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    if long.len() - short.len() > 1 {
        return false;
    }
    let mut i = 0;
    while i < short.len() && short[i] == long[i] {
        i += 1;
    }
    if i == short.len() {
        return true; // equal, or one extra character at the end
    }
    if short.len() == long.len() {
        // One substitution, or one swap of neighbours (paypla).
        short[i + 1..] == long[i + 1..]
            || (i + 1 < short.len()
                && short[i] == long[i + 1]
                && short[i + 1] == long[i]
                && short[i + 2..] == long[i + 2..])
    } else {
        short[i..] == long[i + 1..]
    }
}

/// `true` when `page` resembles the saved site but is a different site:
/// a typo or homoglyph of its name (`paypa1.com`, `pаypal.com`), the same
/// name under another suffix (`paypal.co` for `paypal.com`), or the name
/// with words added (`paypal-secure.com`, `secure-paypal.net`).
///
/// This only drives a warning. Matching ([`match_sites`]) already refuses
/// these pages.
pub fn is_lookalike(saved: &SiteInfo, page: &SiteInfo) -> bool {
    if saved.site == page.site {
        return false;
    }
    let (Some(sb), Some(pb)) = (brand(saved), brand(page)) else {
        return false;
    };
    if sb.chars().count() < 4 {
        // Too short to tell look-alikes from unrelated names (ab.com, ac.com).
        return sb == pb;
    }
    let (ss, ps) = (skeleton(&sb), skeleton(&pb));
    if ss == ps {
        return true;
    }
    if ss.chars().count() >= 5 && edit_distance_at_most_one(&ss, &ps) {
        return true;
    }
    // The brand as a whole word inside a longer name: paypal-login, mypaypal-help.
    pb.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .any(|w| skeleton(w) == ss)
        && pb.contains('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(saved: &str, page: &str) -> SiteMatch {
        match_url(saved, page)
    }

    #[test]
    fn same_site_subdomains_match() {
        assert_eq!(
            m("https://example.com", "https://example.com/login"),
            SiteMatch::Origin
        );
        assert_eq!(
            m("https://example.com", "https://www.example.com"),
            SiteMatch::Site
        );
        assert_eq!(
            m("https://login.example.co.uk/x", "https://example.co.uk"),
            SiteMatch::Site
        );
        assert_eq!(
            m("example.com", "https://accounts.example.com/"),
            SiteMatch::Site
        );
        assert_eq!(
            m("https://EXAMPLE.com.", "https://example.com"),
            SiteMatch::Origin
        );
        assert_eq!(
            m("https://example.com", "https://example.com:8443/"),
            SiteMatch::Host
        );
    }

    #[test]
    fn other_sites_never_match() {
        for page in [
            "https://example.com.evil.io/login",
            "https://example-login.com",
            "https://evilexample.com",
            "https://example.co.uk",
            "https://example.com@evil.io/",
            "https://evil.io/?next=https://example.com",
            "https://evil.io/#example.com",
        ] {
            assert_eq!(m("https://example.com", page), SiteMatch::None, "{page}");
        }
    }

    #[test]
    fn public_and_private_suffixes_are_not_sites() {
        // Private section of the PSL: each user gets their own site.
        assert_eq!(
            m("https://alice.github.io", "https://bob.github.io"),
            SiteMatch::None
        );
        assert_eq!(
            m("https://a.blogspot.com", "https://b.blogspot.com"),
            SiteMatch::None
        );
        assert_eq!(m("https://a.co.uk", "https://b.co.uk"), SiteMatch::None);
        assert_eq!(m("https://co.uk", "https://b.co.uk"), SiteMatch::None);
    }

    #[test]
    fn no_https_downgrade() {
        assert_eq!(
            m("https://example.com", "http://example.com"),
            SiteMatch::None
        );
        assert_eq!(
            m("https://example.com", "http://www.example.com"),
            SiteMatch::None
        );
        assert_eq!(
            m("http://example.com", "https://example.com"),
            SiteMatch::Host
        );
        assert_eq!(
            m("http://example.com", "http://example.com"),
            SiteMatch::Origin
        );
    }

    #[test]
    fn ips_and_local_names_need_same_host_and_port() {
        assert_eq!(
            m("http://192.168.1.1", "http://192.168.1.1/admin"),
            SiteMatch::Origin
        );
        assert_eq!(
            m("http://192.168.1.1", "http://192.168.1.2"),
            SiteMatch::None
        );
        assert_eq!(
            m("http://192.168.1.1:8080", "http://192.168.1.1:9090"),
            SiteMatch::None
        );
        assert_eq!(
            m("http://localhost:3000", "http://localhost:3000/x"),
            SiteMatch::Origin
        );
        assert_eq!(
            m("http://localhost:3000", "http://localhost:4000"),
            SiteMatch::None
        );
        assert_eq!(
            m("http://a.localhost", "http://b.localhost"),
            SiteMatch::None
        );
        assert_eq!(m("https://nas.lan", "https://router.lan"), SiteMatch::None);
        assert_eq!(m("https://nas.lan", "https://nas.lan/"), SiteMatch::Origin);
        assert_eq!(
            m("http://[::1]:8080", "http://[::1]:8080/"),
            SiteMatch::Origin
        );
    }

    #[test]
    fn non_web_pages_never_match() {
        for page in [
            "file:///etc/passwd",
            "data:text/html,<form>",
            "javascript:alert(1)",
            "blob:https://example.com/123",
            "chrome-extension://abc/popup.html",
            "about:blank",
            "",
            "not a url",
        ] {
            assert_eq!(m("https://example.com", page), SiteMatch::None, "{page}");
        }
        assert_eq!(
            m("androidapp://com.example", "https://example.com"),
            SiteMatch::None
        );
        assert_eq!(m("", "https://example.com"), SiteMatch::None);
    }

    #[test]
    fn homoglyph_hosts_are_different_sites() {
        // Cyrillic а (U+0430).
        assert_eq!(
            m("https://paypal.com", "https://pаypal.com"),
            SiteMatch::None
        );
        let p = page_site("https://pаypal.com/").unwrap();
        assert!(p.host.starts_with("xn--"));
        assert_eq!(p.display_host, "pаypal.com");
        // International names match themselves, in either spelling.
        assert_eq!(
            m("https://bücher.de", "https://www.xn--bcher-kva.de"),
            SiteMatch::Site
        );
        assert_eq!(
            page_site("https://www.bücher.de").unwrap().site,
            "xn--bcher-kva.de"
        );
    }

    #[test]
    fn lookalikes() {
        let saved = saved_site("https://www.paypal.com").unwrap();
        let yes = [
            "https://paypa1.com",
            "https://pаypal.com",
            "https://paypal.co",
            "https://paypal-secure.com/login",
            "https://secure-paypal.net",
            "https://paypall.com",
            "https://paypla.com",
            "https://rnicrosoft.com",
        ];
        let ms = saved_site("https://microsoft.com").unwrap();
        for u in yes {
            let p = page_site(u).unwrap();
            let s = if u.contains("rnicro") { &ms } else { &saved };
            assert!(is_lookalike(s, &p), "{u}");
            assert_eq!(match_sites(s, &p), SiteMatch::None, "{u}");
        }
        for u in [
            "https://www.paypal.com/signin",
            "https://example.com",
            "https://paypalfan.blog",
            "https://pay.com",
            "https://alice.github.io",
        ] {
            assert!(!is_lookalike(&saved, &page_site(u).unwrap()), "{u}");
        }
        // Short names: only the exact name elsewhere.
        let ab = saved_site("https://ab.com").unwrap();
        assert!(is_lookalike(&ab, &page_site("https://ab.net").unwrap()));
        assert!(!is_lookalike(&ab, &page_site("https://ac.com").unwrap()));
    }

    #[test]
    fn edit_distance() {
        assert!(edit_distance_at_most_one("paypal", "paypal"));
        assert!(edit_distance_at_most_one("paypal", "paypa"));
        assert!(edit_distance_at_most_one("paypal", "paypxl"));
        assert!(edit_distance_at_most_one("paypal", "paypla"));
        assert!(edit_distance_at_most_one("paypal", "xpaypal"));
        assert!(!edit_distance_at_most_one("paypal", "pypla"));
        assert!(!edit_distance_at_most_one("paypal", "google"));
        assert!(edit_distance_at_most_one("", "a"));
    }
}
