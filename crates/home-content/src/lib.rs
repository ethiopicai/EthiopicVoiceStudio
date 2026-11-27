//! Informational HTML only: native fetch/cache, sanitized fragments, scriptless frame documents.
use ammonia::{Builder, UrlRelative};
use reqwest::{blocking::Client, header, redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const MAX_HTML: usize = 256 * 1024;
pub const FRAME_CSP:&str="default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src 'none'; connect-src 'none'; media-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
const BUNDLED: &str = include_str!("../../../apps/desktop/ui/src/home-fallback.html");
const REFRESH_AFTER: u64 = 24 * 60 * 60;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Only our HTTPS origins can supply HTML. Redirects are never followed.
pub fn validate_endpoint(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "Invalid Home endpoint")?;
    let host = url.host_str().ok_or("Invalid Home endpoint")?;
    if url.scheme() != "https"
        || !(host == "ethiopic.ai" || host.ends_with(".ethiopic.ai"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return Err("Home content requires an approved first-party HTTPS endpoint".into());
    }
    Ok(url)
}

/// Never render endpoint/cache HTML directly in the application DOM.
pub fn sanitize_fragment(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() || raw.len() > MAX_HTML {
        return Err("Home page is empty or exceeds its size limit".into());
    }
    let tags = HashSet::from([
        "article",
        "section",
        "header",
        "footer",
        "main",
        "nav",
        "div",
        "span",
        "p",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "ul",
        "ol",
        "li",
        "table",
        "thead",
        "tbody",
        "tfoot",
        "tr",
        "td",
        "th",
        "hr",
        "br",
        "strong",
        "em",
        "b",
        "i",
        "s",
        "u",
        "code",
        "pre",
        "blockquote",
        "a",
        "img",
        "style",
        "details",
        "summary",
        "small",
        "time",
        "figure",
        "figcaption",
    ]);
    let attrs = HashMap::from([
        ("img", HashSet::from(["src", "alt", "width", "height"])),
        ("details", HashSet::from(["open"])),
        ("td", HashSet::from(["colspan", "rowspan"])),
        ("th", HashSet::from(["colspan", "rowspan", "scope"])),
    ]);
    let clean = Builder::default()
        .rm_clean_content_tags(["style"])
        .tags(tags)
        .tag_attributes(attrs)
        .generic_attributes(HashSet::from([
            "class",
            "id",
            "style",
            "title",
            "lang",
            "dir",
            "aria-label",
        ]))
        .add_clean_content_tags([
            "script", "iframe", "object", "embed", "template", "noscript",
        ])
        .url_schemes(HashSet::from(["data"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(None)
        .strip_comments(true)
        .clean(raw)
        .to_string();
    if clean.trim().is_empty() || clean.len() > MAX_HTML {
        return Err("Home page has no supported content".into());
    }
    Ok(clean)
}

pub fn frame_document(fragment: &str) -> String {
    format!("<!doctype html><html><head><meta http-equiv=\"Content-Security-Policy\" content=\"{FRAME_CSP}\"><meta name=\"referrer\" content=\"no-referrer\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head><body>{fragment}</body></html>")
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomePage {
    pub html: String,
    pub source: String,
    pub configured: bool,
    pub updated_at: Option<u64>,
    pub refresh_due: bool,
    pub notice: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Cache {
    schema: u32,
    url: String,
    html: String,
    fetched_at: u64,
}
#[derive(Deserialize)]
struct Source {
    url: String,
}
pub struct HomeService {
    root: PathBuf,
    attempt: Mutex<Option<Instant>>,
}
impl HomeService {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            attempt: Mutex::new(None),
        }
    }
    fn endpoint(&self) -> Option<Url> {
        let path = self.root.join("source.json");
        if fs::metadata(&path).ok()?.len() > 4096 {
            return None;
        }
        let source: Source = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        validate_endpoint(&source.url).ok()
    }
    fn cache(&self, url: &Url) -> Option<Cache> {
        let mut files = fs::read_dir(self.root.join("cache"))
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .collect::<Vec<_>>();
        files.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
        for file in files.into_iter().take(8) {
            if file.metadata().ok()?.len() > MAX_HTML as u64 * 8 + 16384 {
                continue;
            }
            let Some(bytes) = fs::read(file.path()).ok() else {
                continue;
            };
            let Ok(cache) = serde_json::from_slice::<Cache>(&bytes) else {
                continue;
            };
            if cache.schema == 1
                && cache.url == url.as_str()
                && cache.fetched_at <= now() + 300
                && sanitize_fragment(&cache.html).is_ok()
            {
                return Some(cache);
            }
        }
        None
    }
    pub fn page(&self) -> HomePage {
        let endpoint = self.endpoint();
        if let Some(cache) = endpoint.as_ref().and_then(|url| self.cache(url)) {
            if let Ok(fragment) = sanitize_fragment(&cache.html) {
                return HomePage {
                    html: frame_document(&fragment),
                    source: "cache".into(),
                    configured: true,
                    updated_at: Some(cache.fetched_at),
                    refresh_due: now().saturating_sub(cache.fetched_at) >= REFRESH_AFTER,
                    notice: None,
                };
            }
        }
        let fragment = sanitize_fragment(BUNDLED).expect("bundled Home content must be valid");
        HomePage {
            html: frame_document(&fragment),
            source: "bundled".into(),
            configured: endpoint.is_some(),
            updated_at: None,
            refresh_due: endpoint.is_some(),
            notice: None,
        }
    }
    /// Cache and fallback remain usable when DNS, TLS, server, validation, or disk writes fail.
    pub fn refresh(&self) -> HomePage {
        self.refresh_with(fetch_html)
    }
    fn refresh_with(&self, fetcher: impl FnOnce(&Url) -> Result<String, String>) -> HomePage {
        let Some(url) = self.endpoint() else {
            return self.page();
        };
        let Ok(mut last) = self.attempt.lock() else {
            return self.page();
        };
        if last.is_some_and(|when| when.elapsed() < Duration::from_secs(60)) {
            return self.page();
        }
        *last = Some(Instant::now());
        drop(last);
        match fetcher(&url).and_then(|raw| self.commit(&url, &raw)) {
            Ok(cache) => HomePage {
                html: frame_document(&cache.html),
                source: "remote".into(),
                configured: true,
                updated_at: Some(cache.fetched_at),
                refresh_due: false,
                notice: None,
            },
            Err(_) => {
                let mut page = self.page();
                page.notice = Some("Home updates are unavailable. Using local content.".into());
                page
            }
        }
    }
    fn commit(&self, url: &Url, raw: &str) -> Result<Cache, String> {
        let html = sanitize_fragment(raw)?;
        let cache = Cache {
            schema: 1,
            url: url.as_str().into(),
            html,
            fetched_at: now(),
        };
        let directory = self.root.join("cache");
        fs::create_dir_all(&directory).map_err(|_| "Home cache could not be opened")?;
        let name = format!(
            "{:020}-{}-{}",
            cache.fetched_at,
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::SeqCst)
        );
        let pending = directory.join(format!("{name}.pending"));
        let target = directory.join(format!("{name}.json"));
        let bytes = serde_json::to_vec(&cache).map_err(|_| "Home cache could not be encoded")?;
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&pending)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&pending, &target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(pending);
            return Err("Home content could not be cached".into());
        }
        // Retain the new cache plus one previous version; never remove before commit.
        let mut previous = fs::read_dir(&directory)
            .map_err(|_| "Home cache unavailable")?
            .filter_map(Result::ok)
            .filter(|e| e.path() != target && e.file_name().to_string_lossy().ends_with(".json"))
            .collect::<Vec<_>>();
        previous.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
        for old in previous.into_iter().skip(1) {
            let _ = fs::remove_file(old.path());
        }
        Ok(cache)
    }
}
fn fetch_html(url: &Url) -> Result<String, String> {
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .user_agent("EthiopicVoiceStudioHome/1")
        .build()
        .map_err(|_| "Home connection unavailable")?;
    let response = client
        .get(url.clone())
        .header(header::ACCEPT, "text/html")
        .send()
        .map_err(|_| "Home endpoint unavailable")?;
    if response.status() != reqwest::StatusCode::OK
        || !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("text/html")
            })
    {
        return Err("Home endpoint did not return HTML".into());
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_HTML as u64)
    {
        return Err("Home response exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_HTML as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Home response interrupted")?;
    if bytes.len() > MAX_HTML {
        return Err("Home response exceeds size limit".into());
    }
    String::from_utf8(bytes).map_err(|_| "Home content must be UTF-8".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ethiopic-home-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn service(&self) -> HomeService {
            HomeService::new(self.0.clone())
        }
        fn configure(&self) {
            fs::write(
                self.0.join("source.json"),
                r#"{"url":"https://ethiopic.ai/studio/home"}"#,
            )
            .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn endpoint_is_first_party_https_only() {
        for url in [
            "http://ethiopic.ai/home",
            "https://ethiopic.ai.evil.test/",
            "https://evil.test/",
            "https://user:secret@ethiopic.ai/",
            "file:///etc/passwd",
            "https://127.0.0.1/",
            "https://ethiopic.ai:444/",
            "https://ethiopic.ai/#jump",
        ] {
            assert!(validate_endpoint(url).is_err(), "{url}");
        }
        assert!(validate_endpoint("https://content.ethiopic.ai/home").is_ok());
    }
    #[test]
    fn sanitizes_execution_navigation_forms_and_remote_assets() {
        let clean=sanitize_fragment(r#"<meta http-equiv="refresh" content="0;url=https://evil.test"><base href="https://evil.test"><script>window.__TAURI_INTERNALS__.invoke('get_transcript')</script><iframe srcdoc="evil"></iframe><object data="file:///etc/passwd"></object><a href="https://evil.test" target="_top">Link</a><form action="https://evil.test"><input name="secret"></form><img src="https://evil.test/pixel" srcset="file:///x 2x" onerror="alert(1)"><div onclick="alert(1)">Safe text</div>"#).unwrap();
        for forbidden in [
            "<meta",
            "<base",
            "<script",
            "<iframe",
            "<object",
            "<form",
            "<input",
            "href=",
            "target=",
            "srcset=",
            "onerror=",
            "onclick=",
            "__TAURI_INTERNALS__",
            "https://evil.test",
        ] {
            assert!(!clean.contains(forbidden), "{forbidden}: {clean}");
        }
        assert!(clean.contains("Safe text"));
    }
    #[test]
    fn retains_static_design_but_injects_policy_before_content() {
        let clean=sanitize_fragment("<style>.card{color:#37d39c}</style><section class='card' style='padding:12px'><h2>News</h2><p>ሰላም</p></section>").unwrap();
        assert!(clean.contains("<style>"));
        assert!(clean.contains("ሰላም"));
        let doc = frame_document(&clean);
        assert!(doc.find("Content-Security-Policy").unwrap() < doc.find("<style>").unwrap());
        assert!(doc.contains("connect-src 'none'"));
        assert!(doc.contains("script-src 'none'"));
    }
    #[test]
    fn cache_is_revalidated_and_independent_of_network() {
        let f = Fixture::new();
        let service = f.service();
        assert_eq!(service.page().source, "bundled");
        assert!(!service.page().configured);
        f.configure();
        let url = validate_endpoint("https://ethiopic.ai/studio/home").unwrap();
        service.commit(&url, "<h2>Cached update</h2>").unwrap();
        let reopened = f.service().page();
        assert_eq!(reopened.source, "cache");
        assert!(reopened.html.contains("Cached update"));
        assert!(!reopened.refresh_due);
        assert!(service.commit(&url, &"x".repeat(MAX_HTML + 1)).is_err());
        assert!(service.page().html.contains("Cached update"));
    }
    #[test]
    fn corrupt_cache_and_malformed_config_fall_back() {
        let f = Fixture::new();
        f.configure();
        fs::create_dir_all(f.0.join("cache")).unwrap();
        fs::write(f.0.join("cache/999999.json"), "broken").unwrap();
        assert_eq!(f.service().page().source, "bundled");
        fs::write(f.0.join("source.json"), r#"{"url":"https://evil.test/"}"#).unwrap();
        assert!(!f.service().page().configured);
    }
    #[test]
    fn cache_cannot_introduce_navigation_after_restart() {
        let f = Fixture::new();
        f.configure();
        fs::create_dir_all(f.0.join("cache")).unwrap();
        let cache = Cache {
            schema: 1,
            url: "https://ethiopic.ai/studio/home".into(),
            html: "<p>News</p><a href='file:///x'>bad</a><script>evil()</script>".into(),
            fetched_at: now(),
        };
        fs::write(
            f.0.join("cache/0001.json"),
            serde_json::to_vec(&cache).unwrap(),
        )
        .unwrap();
        let page = f.service().page();
        assert_eq!(page.source, "cache");
        assert!(!page.html.contains("file:///x"));
        assert!(!page.html.contains("evil()"));
    }
    #[test]
    fn refresh_failure_keeps_valid_cache_and_rate_limits_retries() {
        let f = Fixture::new();
        f.configure();
        let service = f.service();
        let url = validate_endpoint("https://ethiopic.ai/studio/home").unwrap();
        service
            .commit(&url, "<p>Keep this update offline</p>")
            .unwrap();
        let page = service.refresh_with(|_| Err("offline".into()));
        assert_eq!(page.source, "cache");
        assert!(page.notice.is_some());
        assert!(page.html.contains("Keep this update offline"));
        service.refresh_with(|_| panic!("retry must be throttled"));
    }
    #[test]
    fn successful_refresh_is_cached_but_invalid_payload_cannot_replace_it() {
        let f = Fixture::new();
        f.configure();
        let page = f
            .service()
            .refresh_with(|_| Ok("<h2>New release</h2><script>bad()</script>".into()));
        assert_eq!(page.source, "remote");
        assert!(!page.html.contains("bad()"));
        let page = f
            .service()
            .refresh_with(|_| Ok("<script>nothing safe</script>".into()));
        assert_eq!(page.source, "cache");
        assert!(page.html.contains("New release"));
        assert!(page.notice.is_some());
        let unconfigured = Fixture::new();
        assert!(
            !unconfigured
                .service()
                .refresh_with(|_| panic!("no configured endpoint must mean no network"))
                .configured
        );
    }
    #[test]
    fn daily_refresh_is_due_only_for_stale_cache() {
        let f = Fixture::new();
        f.configure();
        fs::create_dir_all(f.0.join("cache")).unwrap();
        let mut cache = Cache {
            schema: 1,
            url: "https://ethiopic.ai/studio/home".into(),
            html: "<p>Daily content</p>".into(),
            fetched_at: now() - 60,
        };
        let path = f.0.join("cache/0001.json");
        fs::write(&path, serde_json::to_vec(&cache).unwrap()).unwrap();
        assert!(!f.service().page().refresh_due);
        cache.fetched_at = now() - REFRESH_AFTER - 1;
        fs::write(path, serde_json::to_vec(&cache).unwrap()).unwrap();
        assert!(f.service().page().refresh_due);
    }
}
