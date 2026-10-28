//! The website check: does the page answer with the right status, how fast, and (for https) how long the certificate
//! is still good.
//!
//! The request is a plain GET that does not follow redirects, so a site that answers `301` is reported as `301`
//! rather than as whatever it redirects to. The certificate's end date is read from a separate TLS handshake that
//! accepts any certificate (an expired one is exactly what this has to report), and nothing is sent over it.
//! The URL is not in any error text: it may carry a token in its query.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::net::TcpStream;

/// How much of a page is read when it must contain some text.
const MAX_BODY: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct HttpResult {
    /// The status code, if the site answered at all.
    pub status: Option<u16>,
    /// The right status, and the page had the text it should.
    pub ok: bool,
    /// Time until the answer began, in milliseconds.
    pub time_ms: Option<u64>,
    /// Days until the certificate ends (negative: it ended). Only for https.
    pub cert_days: Option<i64>,
    pub error: Option<String>,
}

/// 0 means "any 2xx or 3xx"; otherwise exactly that code.
pub fn status_ok(expect: u16, got: u16) -> bool {
    if expect == 0 {
        (200..400).contains(&got)
    } else {
        got == expect
    }
}

/// Where a URL points: host, port, and whether it is https. `None` for anything that isn't a plain
/// `http(s)://host[:port]/…` address (credentials in the address are refused).
pub fn target_of(url: &str) -> Option<(String, u16, bool)> {
    if url.len() > 2048 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        (false, url.strip_prefix("http://")?)
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let default_port = if https { 443 } else { 80 };
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let (h, after) = inner.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => p.parse().ok()?,
            Some(_) => return None,
            None if after.is_empty() => default_port,
            None => return None,
        };
        (h.to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            // An address with more than one colon needs brackets; `+80` is not a port.
            Some((h, p)) if !h.contains(':') && !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h.to_string(), p.parse().ok()?),
            Some(_) => return None,
            None => (authority.to_string(), default_port),
        }
    };
    if host.is_empty() || port == 0 || !super::ping::valid_host(&host) {
        return None;
    }
    Some((host, port, https))
}

pub fn valid_url(url: &str) -> bool {
    target_of(url).is_some()
}

/// The URL as shown to people and used in a state's name: no scheme, no query, a sensible length.
pub fn short(url: &str) -> String {
    let s = url.trim_start_matches("https://").trim_start_matches("http://");
    s.split(['?', '#']).next().unwrap_or_default().chars().take(60).collect()
}

fn describe(e: &(dyn std::error::Error + 'static)) -> String {
    let mut text = e.to_string();
    let mut cur = e.source();
    while let Some(inner) = cur {
        let t = inner.to_string();
        if !text.contains(&t) {
            text.push_str(": ");
            text.push_str(&t);
        }
        cur = inner.source();
    }
    text
}

fn failed(error: String) -> HttpResult {
    HttpResult { status: None, ok: false, time_ms: None, cert_days: None, error: Some(error) }
}

/// Check one address. Never errors: a site that does not answer is a result.
pub async fn http_check(url: &str, expect_status: u16, contains: Option<&str>, limit: Duration) -> HttpResult {
    let Some((host, port, https)) = target_of(url) else {
        return failed("that is not an http:// or https:// address".into());
    };
    // rustls has two providers built in; say which one before the first client is made.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(limit)
        .timeout(limit)
        .user_agent(concat!("SSHVault-monitor/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => return failed(format!("could not start the web client: {}", describe(&e.without_url()))),
    };
    let start = Instant::now();
    // The response time is the page's own: not held up by the certificate probe running beside it.
    let (answer, cert_days) = tokio::join!(
        async {
            let r = client.get(url).send().await;
            (r, start.elapsed())
        },
        async {
            if https {
                // One deadline for the whole probe (connecting and the handshake).
                tokio::time::timeout(limit, cert_days_left(&host, port)).await.ok().flatten()
            } else {
                None
            }
        }
    );
    let (answer, took) = answer;
    let res = match answer {
        Ok(r) => r,
        Err(e) => {
            let e = e.without_url();
            let why = if e.is_timeout() {
                format!("the site did not answer within {} s", limit.as_secs().max(1))
            } else if e.is_connect() {
                format!("could not connect ({})", describe(&e))
            } else {
                format!("the request failed ({})", describe(&e))
            };
            return HttpResult { status: None, ok: false, time_ms: None, cert_days, error: Some(why) };
        }
    };
    let time_ms = Some(took.as_millis() as u64);
    let status = res.status().as_u16();
    if !status_ok(expect_status, status) {
        let error = if expect_status == 0 { format!("the site answered {status}") } else { format!("the site answered {status}, not {expect_status}") };
        return HttpResult { status: Some(status), ok: false, time_ms, cert_days, error: Some(error) };
    }
    if let Some(text) = contains.filter(|t| !t.is_empty()) {
        let mut res = res;
        let mut body: Vec<u8> = Vec::new();
        let read = tokio::time::timeout(limit, async {
            loop {
                match res.chunk().await {
                    Ok(Some(chunk)) => {
                        body.extend_from_slice(&chunk);
                        if body.len() >= MAX_BODY {
                            return true;
                        }
                    }
                    Ok(None) => return true,
                    Err(_) => return false,
                }
            }
        })
        .await;
        let page = String::from_utf8_lossy(&body);
        if !page.contains(text) {
            let why = match read {
                Ok(true) if body.len() >= MAX_BODY => "the text was not in the first 256 KB of the page",
                Ok(true) => "the page does not contain the expected text",
                _ => "the page could not be read in time",
            };
            return HttpResult { status: Some(status), ok: false, time_ms, cert_days, error: Some(why.into()) };
        }
    }
    HttpResult { status: Some(status), ok: true, time_ms, cert_days, error: None }
}

// -- the certificate ---------------------------------------------------------------------------------------------

/// Days from the epoch to a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// One DER element: its tag, its content, and what follows it.
fn tlv(b: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *b.first()?;
    let l0 = *b.get(1)?;
    let (len, header) = if l0 < 0x80 {
        (usize::from(l0), 2)
    } else {
        let n = usize::from(l0 & 0x7f);
        if n == 0 || n > 4 {
            return None;
        }
        let mut v = 0usize;
        for i in 0..n {
            v = (v << 8) | usize::from(*b.get(2 + i)?);
        }
        (v, 2 + n)
    };
    let end = header.checked_add(len)?;
    Some((tag, b.get(header..end)?, b.get(end..)?))
}

/// `YYMMDDHHMMSSZ` (UTCTime) or `YYYYMMDDHHMMSSZ` (GeneralizedTime) as seconds since the epoch.
fn parse_time(tag: u8, b: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(b).ok()?;
    let (year, rest): (i64, &str) = match tag {
        0x17 => {
            let yy: i64 = s.get(..2)?.parse().ok()?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, s.get(2..)?)
        }
        0x18 => (s.get(..4)?.parse().ok()?, s.get(4..)?),
        _ => return None,
    };
    let num = |at: usize| -> Option<i64> { rest.get(at..at + 2)?.parse().ok() };
    let (month, day, hour, minute) = (num(0)?, num(2)?, num(4)?, num(6)?);
    let second = num(8).unwrap_or(0);
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

/// When a certificate (DER) stops being valid, as seconds since the epoch.
pub fn not_after_secs(cert: &[u8]) -> Option<i64> {
    let (tag, body, _) = tlv(cert)?;
    if tag != 0x30 {
        return None;
    }
    let (tag, tbs, _) = tlv(body)?;
    if tag != 0x30 {
        return None;
    }
    // [0] version is optional; the serial number follows it.
    let (first, _, after_first) = tlv(tbs)?;
    let after_serial = if first == 0xA0 { tlv(after_first)?.2 } else { after_first };
    let after_algorithm = tlv(after_serial)?.2;
    let after_issuer = tlv(after_algorithm)?.2;
    let (tag, validity, _) = tlv(after_issuer)?;
    if tag != 0x30 {
        return None;
    }
    let after_not_before = tlv(validity)?.2;
    let (tag, not_after, _) = tlv(after_not_before)?;
    parse_time(tag, not_after)
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Whole days until the server's certificate ends, from a handshake that accepts whatever it is shown.
async fn cert_days_left(host: &str, port: u16) -> Option<i64> {
    let tcp = TcpStream::connect((host, port)).await.ok()?;
    let config = crate::db::tlsconf::client_config(crate::db::TlsMode::Require).ok().flatten()?;
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).ok()?;
    let tls = tokio_rustls::TlsConnector::from(config).connect(name, tcp).await.ok()?;
    let (_, conn) = tls.get_ref();
    let leaf = conn.peer_certificates()?.first()?;
    let end = not_after_secs(leaf.as_ref())?;
    Some((end - now_secs()).div_euclid(86_400))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::http::fake;
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    // Two real certificates made with openssl: one ending in 2036 (the two-digit-year form of the date) and one in 2059
    // (the four-digit form, used from 2050 on).
    const CERT_2036: &str = "MIIBhjCCASugAwIBAgIUT+0Umr7/cPQUZssNhLUnesn0sCgwCgYIKoZIzj0EAwIwGDEWMBQGA1UEAwwNc3NodmF1bHQudGVzdDAeFw0yNjEwMjgwNTQzNTlaFw0zNjEwMjUwNTQzNTlaMBgxFjAUBgNVBAMMDXNzaHZhdWx0LnRlc3QwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAR6qB7z8q7wfmLHiCPO5+Ie6ZaZN507H2otyWZ+ngh2m25cQVOXFpOVnVz7SWHzUcj4JzwKrugSmB+sb7z+oCNKo1MwUTAdBgNVHQ4EFgQUlfn5Nl3Wy6SeDp1lglvyAtPBEQEwHwYDVR0jBBgwFoAUlfn5Nl3Wy6SeDp1lglvyAtPBEQEwDwYDVR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNJADBGAiEAo/GpZaNe1D02u2pNE0GaGQprJ/z3nx1OTnSyLX8muUACIQDKKE/Hodnv14084g3clPaEf/XGt9iIEwUbyn7gcuqrng==";
    const CERT_2059: &str = "MIIBiDCCAS2gAwIBAgIUAKYP0ZWoHhjiaZaXsqIJEZl9FDwwCgYIKoZIzj0EAwIwGDEWMBQGA1UEAwwNc3NodmF1bHQudGVzdDAgFw0yNjEwMjgwNTQzNTlaGA8yMDU5MDkwNTA1NDM1OVowGDEWMBQGA1UEAwwNc3NodmF1bHQudGVzdDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABIqu8sPK2lufuAc9e/mFxbchB52q/Lfq2i2c6tpHDlAa2FqjeJnjI7R/2dWhjWQnIz0LdxBTmr2thcKLNk8EhXOjUzBRMB0GA1UdDgQWBBTglZVPz4BlUgMGyxva+VW757a1xjAfBgNVHSMEGDAWgBTglZVPz4BlUgMGyxva+VW757a1xjAPBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0kAMEYCIQCCLTKlHUoFRcuWTlzv5wGwWyGyqqbYd9WFw8OHJ5fi1wIhAPBa+p1zK+9oA2pPz7xEMqs9es0RJU5+ixFik2y4X72V";

    #[test]
    fn reads_the_end_date_of_a_real_certificate_in_both_date_forms() {
        let a = B64.decode(CERT_2036).unwrap();
        assert_eq!(not_after_secs(&a), Some(2_108_526_239), "UTCTime");
        let b = B64.decode(CERT_2059).unwrap();
        assert_eq!(not_after_secs(&b), Some(2_829_966_239), "GeneralizedTime");
    }

    #[test]
    fn nonsense_is_not_a_certificate_and_never_panics() {
        for bytes in [&[][..], &[0x30][..], &[0x30, 0x84, 0xff, 0xff, 0xff, 0xff][..], &[0x30, 0x03, 0x01, 0x01, 0xff][..], b"not a certificate at all"] {
            assert_eq!(not_after_secs(bytes), None);
        }
        let mut cut = B64.decode(CERT_2036).unwrap();
        cut.truncate(120);
        assert_eq!(not_after_secs(&cut), None);
    }

    #[test]
    fn civil_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 10, 28), 20_754);
    }

    #[test]
    fn addresses_the_check_accepts() {
        assert_eq!(target_of("https://example.com/path?x=1"), Some(("example.com".into(), 443, true)));
        assert_eq!(target_of("http://example.com"), Some(("example.com".into(), 80, false)));
        assert_eq!(target_of("http://10.0.0.5:8080/health"), Some(("10.0.0.5".into(), 8080, false)));
        assert_eq!(target_of("https://[::1]:8443/"), Some(("::1".into(), 8443, true)));
        for bad in ["http://::1/", "http://a:b:80/", "http://host:+80/", "http://host:/", "https://[::1]:+1/"] {
            assert!(target_of(bad).is_none(), "{bad:?}");
        }
        for bad in ["", "example.com", "ftp://example.com", "https://", "https://user:pw@example.com/", "http://a b/", "http://exa mple.com", "http://host:0/", "http://host:99999/", "http://-oops/", "https://[::1"] {
            assert!(target_of(bad).is_none(), "{bad:?}");
        }
        assert!(valid_url("https://example.com"));
        assert!(!valid_url("javascript:alert(1)"));
    }

    #[test]
    fn a_short_name_hides_the_query() {
        assert_eq!(short("https://example.com/status?token=SECRET#x"), "example.com/status");
        assert_eq!(short(&format!("http://example.com/{}", "a".repeat(100))).chars().count(), 60);
    }

    #[test]
    fn which_statuses_count_as_up() {
        assert!(status_ok(0, 200) && status_ok(0, 204) && status_ok(0, 301) && status_ok(0, 399));
        assert!(!status_ok(0, 404) && !status_ok(0, 500) && !status_ok(0, 199));
        assert!(status_ok(204, 204) && !status_ok(204, 200));
    }

    fn quick() -> Duration {
        Duration::from_secs(5)
    }

    #[tokio::test]
    async fn a_good_page_is_up_and_a_bad_status_says_so() {
        let server = fake::serve(|req| if req.path == "/ok" { (200, "hello world".to_string()) } else { (503, "down".to_string()) }).await;
        let base = format!("http://127.0.0.1:{}", server.port);
        let up = http_check(&format!("{base}/ok"), 0, None, quick()).await;
        assert!(up.ok && up.status == Some(200) && up.time_ms.is_some() && up.error.is_none(), "{up:?}");
        assert_eq!(up.cert_days, None, "plain http has no certificate");
        let down = http_check(&format!("{base}/broken"), 0, None, quick()).await;
        assert!(!down.ok && down.status == Some(503), "{down:?}");
        assert_eq!(down.error.as_deref(), Some("the site answered 503"));
        let wrong = http_check(&format!("{base}/ok"), 204, None, quick()).await;
        assert_eq!(wrong.error.as_deref(), Some("the site answered 200, not 204"));
    }

    #[tokio::test]
    async fn the_page_can_be_required_to_contain_some_text() {
        let server = fake::serve(|_| (200, "<html>Welcome to the shop</html>".to_string())).await;
        let url = format!("http://127.0.0.1:{}/", server.port);
        assert!(http_check(&url, 0, Some("Welcome"), quick()).await.ok);
        let missing = http_check(&url, 0, Some("Maintenance"), quick()).await;
        assert!(!missing.ok && missing.status == Some(200), "{missing:?}");
        assert_eq!(missing.error.as_deref(), Some("the page does not contain the expected text"));
        assert!(http_check(&url, 0, Some(""), quick()).await.ok, "empty text asks for nothing");
    }

    #[tokio::test]
    async fn a_site_that_is_not_there_is_a_result_not_an_error_and_the_url_stays_out_of_it() {
        let r = http_check("http://127.0.0.1:1/secret-path?token=SECRET", 0, None, quick()).await;
        assert!(!r.ok && r.status.is_none() && r.time_ms.is_none(), "{r:?}");
        let text = r.error.unwrap();
        assert!(!text.contains("SECRET") && !text.contains("secret-path"), "{text}");
        let bad = http_check("not a url", 0, None, quick()).await;
        assert!(!bad.ok && bad.error.is_some());
    }

    #[tokio::test]
    async fn a_redirect_is_reported_not_followed() {
        let server = fake::serve(|_| (302, String::new())).await;
        let r = http_check(&format!("http://127.0.0.1:{}/", server.port), 0, None, quick()).await;
        assert_eq!(r.status, Some(302));
        assert!(r.ok, "a redirect counts as up unless a status is asked for");
        let strict = http_check(&format!("http://127.0.0.1:{}/", server.port), 200, None, quick()).await;
        assert!(!strict.ok);
    }
}
