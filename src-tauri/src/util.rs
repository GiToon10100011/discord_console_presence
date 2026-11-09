use std::{
    net::Ipv4Addr,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use url::{Host, Url};

use crate::error::{AppError, Result};

/// Unix time in milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Locks a mutex, recovering the data if another thread panicked while holding it.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Trims `value` and returns `None` when nothing is left.
pub fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Lowercase ASCII slug used for game ids.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "game".to_string()
    } else {
        slug
    }
}

/// Parses a URL and accepts only `http`/`https`.
pub fn parse_http_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim())
        .map_err(|_| AppError::Invalid("올바른 URL이 아닙니다.".into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::Invalid(
            "HTTP/HTTPS URL만 사용할 수 있습니다.".into(),
        ));
    }
    if url.host().is_none() {
        return Err(AppError::Invalid("URL에 호스트가 없습니다.".into()));
    }
    Ok(url)
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    let cgnat = o[0] == 100 && (64..=127).contains(&o[1]);
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || o[0] == 0
        || cgnat)
}

/// `false` for loopback, private, link-local and obviously-local names. Discord's
/// media proxy can only fetch public addresses, and the app should never be
/// pointed at the user's own network by a pasted URL.
pub fn is_public_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => {
            let d = domain.trim_end_matches('.').to_ascii_lowercase();
            !(d == "localhost"
                || d.ends_with(".localhost")
                || d.ends_with(".local")
                || d.ends_with(".internal")
                || d.ends_with(".lan")
                || !d.contains('.'))
        }
        Some(Host::Ipv4(ip)) => is_public_v4(ip),
        Some(Host::Ipv6(ip)) => match ip.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => {
                let first = ip.segments()[0];
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || (first & 0xfe00) == 0xfc00
                    || (first & 0xffc0) == 0xfe80)
            }
        },
        None => false,
    }
}

/// `parse_http_url` plus the public-host check.
pub fn public_http_url(raw: &str) -> Result<Url> {
    let url = parse_http_url(raw)?;
    if !is_public_host(&url) {
        return Err(AppError::Invalid(
            "공개적으로 접근 가능한 주소만 사용할 수 있습니다 (localhost, 사설 IP 불가).".into(),
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_keeps_ascii_words_and_collapses_separators() {
        assert_eq!(slugify("Pokémon Legends: Z-A"), "pok-mon-legends-z-a");
        assert_eq!(slugify("  Elden   Ring "), "elden-ring");
    }

    #[test]
    fn slugify_falls_back_for_non_ascii_names() {
        assert_eq!(slugify("젤다의 전설"), "game");
    }

    #[test]
    fn non_empty_trims_and_drops_blank() {
        assert_eq!(non_empty(Some("  a ".into())), Some("a".into()));
        assert_eq!(non_empty(Some("   ".into())), None);
        assert_eq!(non_empty(None), None);
    }

    #[test]
    fn only_http_and_https_schemes_are_accepted() {
        assert!(parse_http_url("https://example.com/a.png").is_ok());
        assert!(parse_http_url("http://example.com/a.png").is_ok());
        assert!(parse_http_url("file:///etc/passwd").is_err());
        assert!(parse_http_url("javascript:alert(1)").is_err());
        assert!(parse_http_url("ftp://example.com/a.png").is_err());
        assert!(parse_http_url("not a url").is_err());
    }

    #[test]
    fn private_and_local_hosts_are_rejected() {
        for bad in [
            "http://localhost/a.png",
            "http://127.0.0.1/a.png",
            "http://10.0.0.5/a.png",
            "http://192.168.1.10/a.png",
            "http://172.16.0.1/a.png",
            "http://169.254.169.254/latest/meta-data",
            "http://100.64.0.1/a.png",
            "http://[::1]/a.png",
            "http://[fd00::1]/a.png",
            "http://[::ffff:127.0.0.1]/a.png",
            "http://printer.local/a.png",
            "http://intranet/a.png",
        ] {
            assert!(public_http_url(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn public_hosts_are_accepted() {
        for good in [
            "https://upload.wikimedia.org/wikipedia/en/b/b9/Elden_Ring_Box_art.jpg",
            "https://cdn.discordapp.com/app-assets/1/2.png",
            "http://93.184.216.34/a.png",
        ] {
            assert!(public_http_url(good).is_ok(), "{good} should be accepted");
        }
    }
}
