// SPDX-License-Identifier: MIT

//! Bounded network adapter for explicitly requested link enrichment.
//!
//! Network access stays in this migration crate. The shared core receives
//! already-captured bytes and remains fully offline.

use std::{
    collections::HashMap,
    io::Read,
    sync::{Condvar, Mutex},
    time::Duration,
};

use anyhow::{anyhow, Result};
use oia_core::{parse_product_metadata, ImageBytes, ProductMetadata};
use reqwest::{
    blocking::{Client, Response},
    header::{ACCEPT, CONTENT_TYPE, LOCATION, USER_AGENT},
    redirect::Policy,
    StatusCode,
};
use scraper::{Html, Selector};
use url::Url;

const MAX_HTML_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const MAX_PREVIEW_CANDIDATES: usize = 4;
const MAX_FAVICON_CANDIDATES: usize = 4;
const USER_AGENT_VALUE: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Óia/0.1 LinkMetadataMigration";

pub(crate) struct LinkMetadataCapture {
    pub product: Option<ProductMetadata>,
    pub canonical_url: String,
    pub title: Option<String>,
    pub site: Option<String>,
    pub author: Option<String>,
    pub lang: Option<String>,
    pub excerpt: Option<String>,
    pub theme_color: Option<String>,
    pub images: Vec<ImageBytes>,
    pub preview_url: Option<String>,
    pub favicon_url: Option<String>,
}

pub(crate) trait LinkMetadataFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> std::result::Result<LinkMetadataCapture, LinkFetchError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkFetchError {
    Gone,
    Unreachable,
    RejectedResponse,
    ResponseTooLarge,
    NotHtml,
}

impl LinkFetchError {
    pub const fn description(self) -> &'static str {
        match self {
            Self::Gone => "links returning HTTP 404 or 410",
            Self::Unreachable => "links still unreachable after a retry",
            Self::RejectedResponse => "reachable pages that rejected the metadata request",
            Self::ResponseTooLarge => "pages whose HTML exceeded the safety limit",
            Self::NotHtml => "responses that were not HTML pages",
        }
    }

    pub const fn should_remove(self) -> bool {
        matches!(self, Self::Gone | Self::Unreachable)
    }

    pub const fn reached_server(self) -> bool {
        !matches!(self, Self::Unreachable)
    }
}

pub(crate) struct HttpLinkMetadataFetcher {
    client: Client,
    host_limiter: HostLimiter,
}

impl HttpLinkMetadataFetcher {
    pub(crate) fn new() -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .redirect(Policy::none())
            .build()
            .map_err(|error| anyhow!("could not initialize the link metadata client: {error}"))?;
        Ok(Self {
            client,
            host_limiter: HostLimiter::new(2),
        })
    }

    fn fetch_page(&self, url: &str) -> std::result::Result<(String, Url), LinkFetchError> {
        let mut current = Url::parse(url).map_err(|_| LinkFetchError::Unreachable)?;
        for redirect_count in 0..=MAX_REDIRECTS {
            let host = current.host_str().ok_or(LinkFetchError::Unreachable)?;
            let _host_permit = self.host_limiter.acquire(host);
            let mut response = None;
            for _ in 0..2 {
                match self
                    .client
                    .get(current.clone())
                    .header(USER_AGENT, USER_AGENT_VALUE)
                    .header(ACCEPT, "text/html,application/xhtml+xml;q=0.9,*/*;q=0.1")
                    .send()
                {
                    Ok(received) => {
                        response = Some(received);
                        break;
                    }
                    Err(_) => continue,
                }
            }
            let response = response.ok_or(LinkFetchError::Unreachable)?;
            if response.status().is_redirection() {
                if redirect_count == MAX_REDIRECTS {
                    return Err(LinkFetchError::RejectedResponse);
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(LinkFetchError::RejectedResponse)?;
                let next = current
                    .join(location)
                    .map_err(|_| LinkFetchError::RejectedResponse)?;
                if !matches!(next.scheme(), "http" | "https")
                    || !next.username().is_empty()
                    || next.password().is_some()
                {
                    return Err(LinkFetchError::RejectedResponse);
                }
                current = next;
                continue;
            }
            if matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
                return Err(LinkFetchError::Gone);
            }
            if !response.status().is_success() {
                return Err(LinkFetchError::RejectedResponse);
            }
            if response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    let value = value.to_ascii_lowercase();
                    !value.starts_with("text/html")
                        && !value.starts_with("application/xhtml+xml")
                        && !value.starts_with("application/octet-stream")
                })
            {
                return Err(LinkFetchError::NotHtml);
            }
            let final_url = response.url().clone();
            let bytes = bounded_body(response, MAX_HTML_BYTES)
                .map_err(|_| LinkFetchError::RejectedResponse)?;
            if bytes.len() as u64 > MAX_HTML_BYTES {
                return Err(LinkFetchError::ResponseTooLarge);
            }
            return Ok((String::from_utf8_lossy(&bytes).into_owned(), final_url));
        }
        Err(LinkFetchError::RejectedResponse)
    }

    fn fetch_first_image(&self, candidates: &[String]) -> Option<ImageBytes> {
        candidates.iter().find_map(|url| self.fetch_image(url).ok())
    }

    fn fetch_image(&self, url: &str) -> Result<ImageBytes> {
        let mut current = Url::parse(url)?;
        for redirect_count in 0..=MAX_REDIRECTS {
            let host = current
                .host_str()
                .ok_or_else(|| anyhow!("image URL had no host"))?;
            let _host_permit = self.host_limiter.acquire(host);
            let response = self
                .client
                .get(current.clone())
                .header(USER_AGENT, USER_AGENT_VALUE)
                .header(
                    ACCEPT,
                    "image/avif,image/webp,image/png,image/jpeg,image/*;q=0.8,*/*;q=0.1",
                )
                .send()?;
            if response.status().is_redirection() {
                if redirect_count == MAX_REDIRECTS {
                    return Err(anyhow!("image response exceeded the redirect limit"));
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| anyhow!("image redirect omitted its location"))?;
                let next = current.join(location)?;
                if !matches!(next.scheme(), "http" | "https")
                    || !next.username().is_empty()
                    || next.password().is_some()
                {
                    return Err(anyhow!("image redirect target was not a safe HTTP(S) URL"));
                }
                current = next;
                continue;
            }
            if !response.status().is_success() {
                return Err(anyhow!("image response was unsuccessful"));
            }
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("application/octet-stream")
                .split(';')
                .next()
                .unwrap_or("application/octet-stream")
                .trim()
                .to_ascii_lowercase();
            let bytes = bounded_body(response, MAX_IMAGE_BYTES)?;
            if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
                return Err(anyhow!("image response exceeded the safety limit"));
            }
            if !content_type.starts_with("image/") && !looks_like_image(&bytes) {
                return Err(anyhow!("asset response was not an image"));
            }
            return Ok(ImageBytes {
                // The core associates these bytes with the URL declared by the
                // page, not the final redirect target.
                url: url.to_string(),
                content_type,
                bytes,
            });
        }
        Err(anyhow!("image response exceeded the redirect limit"))
    }
}

impl LinkMetadataFetcher for HttpLinkMetadataFetcher {
    fn fetch(&self, url: &str) -> std::result::Result<LinkMetadataCapture, LinkFetchError> {
        let (html, final_url) = self.fetch_page(url)?;
        let extracted = extract_metadata(&html, &final_url);
        let preview = self.fetch_first_image(&extracted.preview_candidates);
        let favicon = self.fetch_first_image(&extracted.favicon_candidates);
        let preview_url = preview.as_ref().map(|image| image.url.clone());
        let favicon_url = favicon.as_ref().map(|image| image.url.clone());
        let mut images = Vec::with_capacity(2);
        if let Some(preview) = preview {
            images.push(preview);
        }
        if let Some(favicon) = favicon {
            if !images.iter().any(|image| image.url == favicon.url) {
                images.push(favicon);
            }
        }
        Ok(LinkMetadataCapture {
            product: parse_product_metadata(&html, final_url.as_str()),
            canonical_url: extracted.canonical_url,
            title: extracted.title,
            site: extracted.site,
            author: extracted.author,
            lang: extracted.lang,
            excerpt: extracted.excerpt,
            theme_color: extracted.theme_color,
            images,
            preview_url,
            favicon_url,
        })
    }
}

fn bounded_body(response: Response, maximum: u64) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum)
    {
        return Ok(vec![0; maximum.saturating_add(1) as usize]);
    }
    let mut bytes = Vec::new();
    response
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(crate) struct HostLimiter {
    maximum: usize,
    active: Mutex<HashMap<String, usize>>,
    available: Condvar,
}

impl HostLimiter {
    pub(crate) fn new(maximum: usize) -> Self {
        Self {
            maximum: maximum.max(1),
            active: Mutex::new(HashMap::new()),
            available: Condvar::new(),
        }
    }

    pub(crate) fn acquire<'a>(&'a self, host: &str) -> HostPermit<'a> {
        let mut active = self.active.lock().expect("host limiter lock poisoned");
        while active.get(host).copied().unwrap_or(0) >= self.maximum {
            active = self
                .available
                .wait(active)
                .expect("host limiter lock poisoned");
        }
        *active.entry(host.to_string()).or_default() += 1;
        HostPermit {
            limiter: self,
            host: host.to_string(),
        }
    }
}

pub(crate) struct HostPermit<'a> {
    limiter: &'a HostLimiter,
    host: String,
}

impl Drop for HostPermit<'_> {
    fn drop(&mut self) {
        let mut active = self
            .limiter
            .active
            .lock()
            .expect("host limiter lock poisoned");
        if let Some(count) = active.get_mut(&self.host) {
            *count -= 1;
            if *count == 0 {
                active.remove(&self.host);
            }
        }
        self.limiter.available.notify_all();
    }
}

struct ExtractedMetadata {
    canonical_url: String,
    title: Option<String>,
    site: Option<String>,
    author: Option<String>,
    lang: Option<String>,
    excerpt: Option<String>,
    theme_color: Option<String>,
    preview_candidates: Vec<String>,
    favicon_candidates: Vec<String>,
}

fn extract_metadata(html: &str, page_url: &Url) -> ExtractedMetadata {
    let document = Html::parse_document(html);
    let meta_selector = Selector::parse("meta").expect("static selector");
    let link_selector = Selector::parse("link[href]").expect("static selector");
    let title_selector = Selector::parse("title").expect("static selector");
    let html_selector = Selector::parse("html").expect("static selector");

    let mut metadata = Vec::<(String, String)>::new();
    for element in document.select(&meta_selector) {
        let key = element
            .value()
            .attr("property")
            .or_else(|| element.value().attr("name"))
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let value = compact(element.value().attr("content"));
        if !key.is_empty() {
            if let Some(value) = value {
                metadata.push((key, value));
            }
        }
    }

    let first_meta = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            metadata
                .iter()
                .find(|(candidate, _)| candidate == key)
                .map(|(_, value)| value.clone())
        })
    };
    let all_meta = |keys: &[&str]| {
        keys.iter()
            .flat_map(|key| {
                metadata
                    .iter()
                    .filter(move |(candidate, _)| candidate == key)
                    .map(|(_, value)| value.clone())
            })
            .collect::<Vec<_>>()
    };

    let canonical_link = document.select(&link_selector).find_map(|element| {
        rel_tokens(element.value().attr("rel"))
            .iter()
            .any(|token| token == "canonical")
            .then(|| element.value().attr("href"))
            .flatten()
            .and_then(|href| http_asset_url(href, page_url))
    });
    let canonical_url = canonical_link
        .or_else(|| first_meta(&["og:url"]).and_then(|url| http_asset_url(&url, page_url)))
        .unwrap_or_else(|| page_url.as_str().to_string());
    let canonical_base = Url::parse(&canonical_url).unwrap_or_else(|_| page_url.clone());
    let title = first_meta(&["og:title", "twitter:title"]).or_else(|| {
        document
            .select(&title_selector)
            .next()
            .and_then(|element| compact(Some(&element.text().collect::<String>())))
    });
    let site =
        first_meta(&["og:site_name"]).or_else(|| canonical_base.host_str().map(str::to_string));
    let author = first_meta(&["author", "article:author"]);
    let excerpt = first_meta(&["og:description", "twitter:description", "description"]);
    let lang = document
        .select(&html_selector)
        .next()
        .and_then(|element| compact(element.value().attr("lang")))
        .or_else(|| first_meta(&["og:locale"]))
        .map(|value| value.replace('_', "-"));
    let theme_color = first_meta(&["theme-color", "theme_color"]);

    let mut preview_candidates = unique_http_urls(
        all_meta(&[
            "og:image:secure_url",
            "og:image",
            "og:image:url",
            "twitter:image",
            "twitter:image:src",
        ]),
        page_url,
    );
    preview_candidates.truncate(MAX_PREVIEW_CANDIDATES);

    let mut favicon_candidates = document
        .select(&link_selector)
        .enumerate()
        .filter_map(|(index, element)| {
            let rel = rel_tokens(element.value().attr("rel"));
            if !rel
                .iter()
                .any(|token| matches!(token.as_str(), "icon" | "apple-touch-icon"))
            {
                return None;
            }
            if element
                .value()
                .attr("type")
                .is_some_and(|value| value.eq_ignore_ascii_case("image/svg+xml"))
            {
                return None;
            }
            let url = http_asset_url(element.value().attr("href")?, page_url)?;
            Some((
                declared_icon_size(element.value().attr("sizes")),
                index,
                url,
            ))
        })
        .collect::<Vec<_>>();
    favicon_candidates.sort_by(|left, right| right.0.cmp(&left.0).then(right.1.cmp(&left.1)));
    let mut favicon_candidates = favicon_candidates
        .into_iter()
        .map(|(_, _, url)| url)
        .collect::<Vec<_>>();
    let mut seen = std::collections::HashSet::new();
    favicon_candidates.retain(|url| seen.insert(url.clone()));
    let default = http_asset_url("/favicon.ico", page_url);
    if let Some(default) = default.as_ref() {
        favicon_candidates.retain(|candidate| candidate != default);
    }
    let reserved_default = if default.is_some() { 1 } else { 0 };
    favicon_candidates.truncate(MAX_FAVICON_CANDIDATES.saturating_sub(reserved_default));
    if let Some(default) = default {
        favicon_candidates.push(default);
    }

    ExtractedMetadata {
        canonical_url,
        title,
        site,
        author,
        lang,
        excerpt,
        theme_color,
        preview_candidates,
        favicon_candidates,
    }
}

fn compact(value: Option<&str>) -> Option<String> {
    let value = value?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty()).then_some(value)
}

fn rel_tokens(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split_ascii_whitespace()
        .map(str::to_ascii_lowercase)
        .collect()
}

fn unique_http_urls(values: Vec<String>, base: &Url) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        if let Some(url) = http_asset_url(&value, base) {
            if !result.contains(&url) {
                result.push(url);
            }
        }
    }
    result
}

fn http_asset_url(value: &str, base: &Url) -> Option<String> {
    let url = base.join(value.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url.into())
}

fn declared_icon_size(value: Option<&str>) -> u64 {
    let value = value.unwrap_or_default();
    if value
        .split_ascii_whitespace()
        .any(|token| token.eq_ignore_ascii_case("any"))
    {
        return u64::MAX;
    }
    value
        .split_ascii_whitespace()
        .filter_map(|token| {
            token
                .to_ascii_lowercase()
                .split_once('x')
                .map(|(a, b)| (a.to_string(), b.to_string()))
        })
        .filter_map(|(width, height)| {
            width
                .parse::<u64>()
                .ok()?
                .checked_mul(height.parse::<u64>().ok()?)
        })
        .max()
        .unwrap_or(0)
}

fn looks_like_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(&[0x00, 0x00, 0x01, 0x00])
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
        || bytes.starts_with(b"BM")
        || bytes.starts_with(b"II*\0")
        || bytes.starts_with(b"MM\0*")
}

#[cfg(test)]
mod tests {
    use std::{io::Write as _, net::TcpListener, thread};

    use super::*;

    #[test]
    fn html_metadata_uses_social_and_icon_priority_with_relative_urls() {
        let page = Url::parse("https://example.com/posts/one").unwrap();
        let extracted = extract_metadata(
            r#"<html lang="en_IE"><head>
                <title>Document title</title>
                <link rel="Canonical" href="/canonical">
                <link rel="ICON" href="/small.ico" sizes="16x16">
                <link rel="Apple-Touch-Icon" href="/touch.png" sizes="180x180">
                <meta property="og:title" content="Social title">
                <meta property="og:image" content="/social.jpg">
                <meta name="theme-color" content="rgb(1 2 3)">
            </head></html>"#,
            &page,
        );

        assert_eq!(extracted.canonical_url, "https://example.com/canonical");
        assert_eq!(extracted.title.as_deref(), Some("Social title"));
        assert_eq!(extracted.lang.as_deref(), Some("en-IE"));
        assert_eq!(extracted.theme_color.as_deref(), Some("rgb(1 2 3)"));
        assert_eq!(
            extracted.preview_candidates,
            vec!["https://example.com/social.jpg"]
        );
        assert_eq!(
            extracted.favicon_candidates,
            vec![
                "https://example.com/touch.png",
                "https://example.com/small.ico",
                "https://example.com/favicon.ico",
            ]
        );
    }

    #[test]
    fn metadata_caps_asset_candidates_and_keeps_the_default_favicon() {
        let page = Url::parse("https://example.com/posts/one").unwrap();
        let extracted = extract_metadata(
            r#"<html><head>
                <meta property="og:image" content="/one.png">
                <meta property="og:image" content="/two.png">
                <meta property="og:image" content="/three.png">
                <meta property="og:image" content="/four.png">
                <meta property="og:image" content="/five.png">
                <link rel="icon" href="/16.png" sizes="16x16">
                <link rel="icon" href="/32.png" sizes="32x32">
                <link rel="icon" href="/64.png" sizes="64x64">
                <link rel="icon" href="/128.png" sizes="128x128">
                <link rel="icon" href="/256.png" sizes="256x256">
            </head></html>"#,
            &page,
        );

        assert_eq!(
            extracted.preview_candidates,
            vec![
                "https://example.com/one.png",
                "https://example.com/two.png",
                "https://example.com/three.png",
                "https://example.com/four.png",
            ]
        );
        assert_eq!(
            extracted.favicon_candidates,
            vec![
                "https://example.com/256.png",
                "https://example.com/128.png",
                "https://example.com/64.png",
                "https://example.com/favicon.ico",
            ]
        );
    }

    #[test]
    fn overflowing_declared_icon_size_is_ignored() {
        assert_eq!(
            declared_icon_size(Some("18446744073709551615x2 32x32")),
            1024
        );
        assert_eq!(declared_icon_size(Some("18446744073709551615x2")), 0);
    }

    #[test]
    fn image_fetch_follows_bounded_redirects_and_keeps_the_declared_url() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        let destination_url = format!("http://{}/image.png", destination.local_addr().unwrap());
        let destination_server = serve_once(
            destination,
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 11\r\nConnection: close\r\n\r\nimage-bytes",
        );

        let redirect = TcpListener::bind("127.0.0.1:0").unwrap();
        let declared_url = format!("http://{}/social.png", redirect.local_addr().unwrap());
        let redirect_server = serve_once(
            redirect,
            &format!(
                "HTTP/1.1 302 Found\r\nLocation: {destination_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );

        let image = HttpLinkMetadataFetcher::new()
            .unwrap()
            .fetch_image(&declared_url)
            .unwrap();

        redirect_server.join().unwrap();
        destination_server.join().unwrap();
        assert_eq!(image.url, declared_url);
        assert_eq!(image.content_type, "image/png");
        assert_eq!(image.bytes, b"image-bytes");
    }

    #[test]
    fn image_fetch_rejects_a_redirect_beyond_the_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/loop.png", listener.local_addr().unwrap());
        let server = serve_redirect_loop(listener, &url, MAX_REDIRECTS + 1);

        let error = HttpLinkMetadataFetcher::new()
            .unwrap()
            .fetch_image(&url)
            .err()
            .expect("the redirect loop should be rejected");

        server.join().unwrap();
        assert!(error.to_string().contains("redirect limit"));
    }

    fn serve_once(listener: TcpListener, response: &str) -> thread::JoinHandle<()> {
        let response = response.as_bytes().to_vec();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            stream.write_all(&response).unwrap();
        })
    }

    fn serve_redirect_loop(
        listener: TcpListener,
        location: &str,
        request_count: usize,
    ) -> thread::JoinHandle<()> {
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
        thread::spawn(move || {
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request).unwrap();
                stream.write_all(&response).unwrap();
            }
        })
    }
}
