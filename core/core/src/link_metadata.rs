// SPDX-License-Identifier: MIT

//! Conservative cultural link presentation from the page's main entity.

use std::{collections::HashMap, time::Duration};

use anyhow::Result;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::product::{
    choose_main_entity, collect_nodes, has_type, is_public_metadata_url, negative_page_context,
    resolved, structured_dom_products, structured_json_roots, MAX_PRODUCT_HTML_BYTES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkCategory {
    Book,
    Music,
    Movie,
}

impl LinkCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Music => "music",
            Self::Movie => "movie",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "book" => Some(Self::Book),
            "music" => Some(Self::Music),
            "movie" => Some(Self::Movie),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedLinkMetadata {
    pub category: LinkCategory,
    pub title: Option<String>,
    pub image_url: Option<String>,
    pub isbn: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DetectedPageMetadata {
    pub link: Option<DetectedLinkMetadata>,
    pub product: Option<crate::ProductMetadata>,
}

/// Inspect structured metadata without network or library access. A title or
/// image never establishes identity. Empty evidence permits only known exact
/// provider routes, so URL-only ingress can still classify those offline.
pub fn parse_link_metadata(html: &str, page_url: &str) -> Option<DetectedLinkMetadata> {
    inspect_link_metadata(html, page_url).ok().flatten()
}

// Rejected evidence differs from a page without an entity: an optional owner
// catalog lookup must never erase a TV, article, review, or boundedness veto.
fn inspect_link_metadata(
    html: &str,
    page_url: &str,
) -> std::result::Result<Option<DetectedLinkMetadata>, ()> {
    if html.len() > MAX_PRODUCT_HTML_BYTES {
        return Err(());
    }
    let page = Url::parse(page_url).map_err(|_| ())?;
    if !is_public_metadata_url(&page) {
        return Err(());
    }
    let document = Html::parse_document(html);
    let mut roots = structured_json_roots(&document).ok_or(())?;
    roots.extend(structured_dom_products(&document).ok_or(())?);
    let mut nodes = Vec::new();
    for root in &roots {
        collect_nodes(root, true, &mut nodes);
    }
    if nodes.len() > crate::product::MAX_STRUCTURED_NODES {
        return Err(());
    }
    let ids: HashMap<_, _> = nodes
        .iter()
        .filter_map(|(node, _)| {
            (node.as_object()?.len() > 1).then_some((node.get("@id")?.as_str()?, *node))
        })
        .collect();
    let og = open_graph(&document);
    if nodes.iter().any(|(node, eligible)| {
        *eligible
            && has_type(node, "WebPage")
            && is_television(node)
            && (crate::product::url_score(node, &page) > 0
                || (node.get("url").is_none() && node.get("@id").is_none()))
    }) {
        return Err(());
    }
    if !negative_page_context(&nodes)
        && !text(og.get("og:type")).is_some_and(|kind| kind == "article")
    {
        if let Some(book) = crate::harvard_book::inspect(&document, &page)? {
            return Ok(Some(book));
        }
    }
    let main = choose_main_entity(&nodes, &ids, &page, |node| {
        entity_category(node).is_some() || is_television(node)
    });
    if let Some(entity) = main {
        let explicit_main = nodes.iter().any(|(node, eligible)| {
            *eligible
                && has_type(node, "WebPage")
                && (crate::product::url_score(node, &page) > 0
                    || (node.get("url").is_none() && node.get("@id").is_none()))
                && node
                    .get("mainEntity")
                    .is_some_and(|main| resolved(main, &ids) == entity)
        });
        if is_television(entity)
            || (!explicit_main
                && nodes.iter().any(|(node, eligible)| {
                    *eligible && is_television(node) && node.get("url").is_none()
                }))
        {
            return Err(());
        }
        let category = entity_category(entity).ok_or(())?;
        let isbn = entity_isbn(entity);
        return Ok(Some(DetectedLinkMetadata {
            category,
            title: text(entity.get("name")).or_else(|| text(og.get("og:title"))),
            image_url: image_url(entity.get("image"), &ids, &page)
                .or_else(|| og_image_url(&og, &page))
                .or_else(|| isbn_cover(category, isbn.as_deref())),
            isbn,
            author: None,
        }));
    }
    // Unscoped competing TV/movie works are ambiguous. A selected Movie with
    // a separate recommended TVSeries is handled above and remains a movie.
    if nodes
        .iter()
        .any(|(node, eligible)| *eligible && is_television(node))
    {
        return Err(());
    }
    if negative_page_context(&nodes)
        || text(og.get("og:type")).is_some_and(|kind| {
            matches!(
                kind.as_str(),
                "article"
                    | "video.tv_show"
                    | "video.episode"
                    | "profile"
                    | "music.musician"
                    | "music.radio_station"
            )
        })
    {
        return Err(());
    }
    let (route_category, route_isbn) = route_category(&page)
        .map(|(category, isbn)| (Some(category), isbn))
        .unwrap_or_default();
    let category = text(og.get("og:type"))
        .and_then(|kind| match kind.as_str() {
            "book" => Some(LinkCategory::Book),
            "music.album" | "music.song" | "music.playlist" => Some(LinkCategory::Music),
            "video.movie" => Some(LinkCategory::Movie),
            _ => None,
        })
        .or(route_category);
    // Conflicting or ambiguous declared works prevent route/OG or catalog
    // fallback from classifying an unrelated recommended entity.
    if nodes
        .iter()
        .any(|(node, eligible)| *eligible && entity_category(node).is_some())
    {
        return Err(());
    }
    let Some(category) = category else {
        return Ok(None);
    };
    let isbn = isbn(og.get("book:isbn")).or(route_isbn);
    Ok(Some(DetectedLinkMetadata {
        category,
        title: text(og.get("og:title")),
        image_url: og_image_url(&og, &page).or_else(|| isbn_cover(category, isbn.as_deref())),
        isbn,
        author: None,
    }))
}

fn isbn_cover(category: LinkCategory, isbn: Option<&str>) -> Option<String> {
    (category == LinkCategory::Book)
        .then_some(isbn?)
        .map(|isbn| format!("https://covers.openlibrary.org/b/isbn/{isbn}-L.jpg?default=false"))
}

fn is_television(node: &Value) -> bool {
    ["TVSeries", "TVEpisode", "TVSeason", "MusicVideoObject"]
        .iter()
        .any(|kind| has_type(node, kind))
}

fn entity_isbn(node: &Value) -> Option<String> {
    isbn(node.get("isbn")).or_else(|| isbn(node.get("gtin13")))
}

fn og_image_url(og: &HashMap<String, Value>, page: &Url) -> Option<String> {
    ["og:image:secure_url", "og:image", "og:image:url"]
        .iter()
        .find_map(|key| image_url(og.get(*key), &HashMap::new(), page))
}

fn entity_category(node: &Value) -> Option<LinkCategory> {
    if has_type(node, "Book") || (has_type(node, "Product") && entity_isbn(node).is_some()) {
        Some(LinkCategory::Book)
    } else if ["MusicAlbum", "MusicRecording", "MusicPlaylist"]
        .iter()
        .any(|kind| has_type(node, kind))
    {
        Some(LinkCategory::Music)
    } else if has_type(node, "Movie") {
        Some(LinkCategory::Movie)
    } else {
        None
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn image_url(value: Option<&Value>, ids: &HashMap<&str, &Value>, page: &Url) -> Option<String> {
    let value = resolved(value?, ids);
    if let Some(values) = value.as_array() {
        return values
            .iter()
            .find_map(|value| image_url(Some(value), ids, page));
    }
    let raw = value.as_str().or_else(|| {
        ["contentUrl", "url"]
            .iter()
            .find_map(|key| value.get(key).and_then(Value::as_str))
    })?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let url = page.join(raw).ok()?;
    is_public_metadata_url(&url).then(|| url.to_string())
}

fn open_graph(document: &Html) -> HashMap<String, Value> {
    let selector = Selector::parse("meta[property],meta[name]").expect("static selector");
    let mut values = HashMap::new();
    let mut image_count = 0;
    for element in document.select(&selector) {
        let Some(key) = element
            .value()
            .attr("property")
            .or_else(|| element.value().attr("name"))
        else {
            continue;
        };
        let Some(content) = element.value().attr("content") else {
            continue;
        };
        let key = key.to_ascii_lowercase();
        if matches!(key.as_str(), "og:image" | "og:image:url") {
            image_count += 1;
        }
        if key.starts_with("og:image:") && image_count > 1 {
            continue;
        }
        values
            .entry(key)
            .or_insert_with(|| Value::String(content.to_string()));
    }
    values
}

fn isbn(value: Option<&Value>) -> Option<String> {
    normalize_isbn(value?.as_str()?)
}

/// Accept only valid ISBN-10/13 check digits, ignoring customary separators.
pub fn normalize_isbn(raw: &str) -> Option<String> {
    let normalized: String = raw
        .chars()
        .filter(|character| !matches!(character, '-' | ' '))
        .collect();
    let bytes = normalized.as_bytes();
    let valid = match bytes.len() {
        13 => {
            bytes.iter().all(u8::is_ascii_digit)
                && (bytes.starts_with(b"978") || bytes.starts_with(b"979"))
                && bytes
                    .iter()
                    .enumerate()
                    .map(|(index, digit)| {
                        u32::from(digit - b'0') * if index % 2 == 0 { 1 } else { 3 }
                    })
                    .sum::<u32>()
                    % 10
                    == 0
        }
        10 => {
            bytes[..9].iter().all(u8::is_ascii_digit)
                && (bytes[9].is_ascii_digit() || matches!(bytes[9], b'X' | b'x'))
                && bytes
                    .iter()
                    .enumerate()
                    .map(|(index, digit)| {
                        let value = if matches!(digit, b'X' | b'x') {
                            10
                        } else {
                            digit - b'0'
                        };
                        u32::from(value) * (10 - index as u32)
                    })
                    .sum::<u32>()
                    % 11
                    == 0
        }
        _ => false,
    };
    valid.then(|| normalized.to_ascii_uppercase())
}

fn route_category(page: &Url) -> Option<(LinkCategory, Option<String>)> {
    let host = page.host_str()?;
    let parts: Vec<_> = page
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let numeric_prefix = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_digit())
            && value
                .split(['-', '.'])
                .next()
                .is_some_and(|id| id.bytes().all(|byte| byte.is_ascii_digit()))
    };
    let book = match (host, parts.as_slice()) {
        ("goodreads.com" | "www.goodreads.com", ["book", "show", id]) => numeric_prefix(id),
        ("openlibrary.org" | "www.openlibrary.org", ["books" | "works", id, ..]) => {
            let suffix = if parts[0] == "books" { 'M' } else { 'W' };
            id.strip_prefix("OL")
                .and_then(|id| id.strip_suffix(suffix))
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
                && parts.len() <= 3
        }
        ("waterstones.com" | "www.waterstones.com", ["book", _, _, raw]) => {
            return normalize_isbn(raw).map(|isbn| (LinkCategory::Book, Some(isbn)));
        }
        ("bookshop.org" | "www.bookshop.org" | "uk.bookshop.org", ["p", "books", slug, id]) => {
            !slug.is_empty()
                && !id.is_empty()
                && (id.bytes().all(|byte| byte.is_ascii_digit())
                    || (id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit())))
        }
        _ => false,
    };
    if book {
        return Some((LinkCategory::Book, None));
    }
    let spotify = parts.first().is_some_and(|part| part.starts_with("intl-"));
    let music_parts = if spotify { &parts[1..] } else { &parts[..] };
    let music = match (host, music_parts) {
        ("open.spotify.com", ["album" | "track" | "playlist", id]) => {
            id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }
        ("music.apple.com", [country, "album" | "song" | "playlist", _, id]) => {
            country.len() == 2
                && country.bytes().all(|byte| byte.is_ascii_alphabetic())
                && ((!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
                    || (id.starts_with("pl.")
                        && id.len() > 3
                        && id.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
                        })))
        }
        (_, ["album" | "track", slug]) if host.ends_with(".bandcamp.com") => {
            !slug.is_empty() && host != "www.bandcamp.com"
        }
        _ => false,
    };
    if music {
        return Some((LinkCategory::Music, None));
    }
    match (host, parts.as_slice()) {
        ("letterboxd.com" | "www.letterboxd.com", ["film", slug]) if !slug.is_empty() => {
            Some((LinkCategory::Movie, None))
        }
        _ => None,
    }
}

/// Fetch one HTML snapshot for both cultural and product metadata. Unavailable
/// HTTP preserves exact known-route classification and never prevents saving.
pub fn fetch_page_metadata(url: &str) -> Result<DetectedPageMetadata> {
    fetch_page_metadata_with(url, crate::product::fetch_public_bytes)
}

type PublicResponse = (Vec<u8>, Option<String>, String);

fn fetch_page_metadata_with(
    url: &str,
    mut fetch: impl FnMut(&str, usize, Duration) -> Result<PublicResponse>,
) -> Result<DetectedPageMetadata> {
    let fallback = parse_link_metadata("", url);
    let title_id = imdb_title_id(url);
    let (mut metadata, catalog_allowed) =
        match fetch(url, MAX_PRODUCT_HTML_BYTES, Duration::from_secs(8)) {
            Ok((bytes, content_type, final_url))
                if crate::product::is_html_content_type(content_type.as_deref()) =>
            {
                let html = String::from_utf8_lossy(&bytes);
                inspect_fetched_page(&html, url, &final_url)
            }
            Ok(_) => (
                DetectedPageMetadata {
                    link: fallback,
                    product: None,
                },
                true,
            ),
            // Evidence truncated by the retrieval cap cannot safely discard a
            // conflicting page context and then classify from its route alone.
            Err(error) if error.to_string().contains("exceeded size limit") => {
                return Ok(DetectedPageMetadata::default());
            }
            Err(error) if fallback.is_none() && title_id.is_none() => return Err(error),
            Err(_) => (
                DetectedPageMetadata {
                    link: fallback,
                    product: None,
                },
                true,
            ),
        };
    if catalog_allowed && metadata.link.is_none() && metadata.product.is_none() {
        if let Some(title_id) = title_id.as_deref() {
            let endpoint = imdb_catalog_url(title_id);
            metadata.link = fetch(&endpoint, MAX_IMDB_CATALOG_BYTES, IMDB_CATALOG_DEADLINE)
                .ok()
                .and_then(|(bytes, content_type, final_url)| {
                    (final_url == endpoint && json_content_type(content_type.as_deref()))
                        .then(|| parse_imdb_catalog(&bytes, title_id))
                        .flatten()
                });
        }
    }
    Ok(metadata)
}

fn inspect_fetched_page(
    html: &str,
    requested_url: &str,
    final_url: &str,
) -> (DetectedPageMetadata, bool) {
    let inspection = inspect_link_metadata(html, final_url);
    let catalog_allowed = inspection.is_ok();
    let link = inspection.ok().flatten().or_else(|| {
        // Same-provider login/challenge redirects may expose no work metadata.
        // Retain only the original exact-route evidence, never the challenge's
        // title or logo. Explicit negative page context still prevents fallback.
        let requested = Url::parse(requested_url).ok()?;
        let final_page = Url::parse(final_url).ok()?;
        (requested != final_page
            && requested.origin() == final_page.origin()
            && parse_link_metadata(html, requested_url).is_some())
        .then(|| parse_link_metadata("", requested_url))
        .flatten()
    });
    (
        DetectedPageMetadata {
            link,
            product: crate::parse_product_metadata(html, final_url),
        },
        catalog_allowed,
    )
}

#[cfg(test)]
fn parse_fetched_page(html: &str, requested_url: &str, final_url: &str) -> DetectedPageMetadata {
    inspect_fetched_page(html, requested_url, final_url).0
}

const MAX_IMDB_CATALOG_BYTES: usize = 128 * 1024;
const IMDB_CATALOG_DEADLINE: Duration = Duration::from_secs(4);

fn imdb_title_id(raw: &str) -> Option<String> {
    let page = Url::parse(raw).ok()?;
    if !is_public_metadata_url(&page)
        || !matches!(page.host_str()?, "imdb.com" | "www.imdb.com")
        || page.fragment().is_some()
        || page.query_pairs().any(|(key, _)| {
            !matches!(key.as_ref(), "ref" | "ref_")
                && !key.starts_with("utm_")
                && !key.starts_with("pf_rd_")
        })
    {
        return None;
    }
    let path = page.path().strip_prefix("/title/")?;
    let id = path.strip_suffix('/').unwrap_or(path);
    let digits = id.strip_prefix("tt")?;
    if !(7..=12).contains(&digits.len()) || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(id.to_string())
}

fn imdb_catalog_url(title_id: &str) -> String {
    format!("https://v3.sg.media-imdb.com/suggestion/t/{title_id}.json")
}

fn json_content_type(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/json")
    })
}

// This is an observed public IMDb-owned catalog response, not a guaranteed
// interface. Missing or changed records remain ordinary links. No credentials,
// HTML challenge handling, or authentication bypass is involved.
fn parse_imdb_catalog(bytes: &[u8], title_id: &str) -> Option<DetectedLinkMetadata> {
    if bytes.len() > MAX_IMDB_CATALOG_BYTES {
        return None;
    }
    let response: Value = serde_json::from_slice(bytes).ok()?;
    let entries = response.get("d")?.as_array()?;
    if entries.len() > 32 {
        return None;
    }
    let mut matching = entries
        .iter()
        .filter(|entry| entry.get("id").and_then(Value::as_str) == Some(title_id));
    let entry = matching.next()?;
    if matching.next().is_some() || entry.get("qid").and_then(Value::as_str) != Some("movie") {
        return None;
    }
    let title = entry.get("l")?.as_str()?.trim();
    if title.is_empty() || title.len() > 2048 || title.chars().count() > 512 {
        return None;
    }
    let image_url = entry
        .get("i")
        .and_then(|image| image.get("imageUrl"))
        .and_then(Value::as_str)
        .filter(|raw| raw.len() <= 8192)
        .and_then(|raw| Url::parse(raw.trim()).ok())
        .filter(|url| url.as_str().len() <= 8192)
        .filter(is_public_metadata_url)
        .map(|url| url.to_string());
    Some(DetectedLinkMetadata {
        category: LinkCategory::Movie,
        title: Some(title.to_string()),
        image_url,
        isbn: None,
        author: None,
    })
}

pub fn fetch_link_metadata(url: &str) -> Result<Option<DetectedLinkMetadata>> {
    Ok(fetch_page_metadata(url)?.link)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(data: &str) -> Option<DetectedLinkMetadata> {
        parse_link_metadata(
            &format!("<script type='application/ld+json'>{data}</script>"),
            "https://example.com/work",
        )
    }

    #[test]
    fn main_book_graph_resolves_name_and_cover() {
        let metadata = parse(r##"{"@graph":[{"@type":"WebPage","url":"https://example.com/work","mainEntity":{"@id":"#book"}},{"@id":"#book","@type":"Book","name":" Book title ","isbn":"978-0-306-40615-7","image":{"@id":"#cover"}},{"@id":"#cover","@type":"ImageObject","contentUrl":"/cover.jpg"},{"@type":"Book","url":"/other","name":"Recommendation"}]}"##).unwrap();
        assert_eq!(metadata.category, LinkCategory::Book);
        assert_eq!(metadata.title.as_deref(), Some("Book title"));
        assert_eq!(
            metadata.image_url.as_deref(),
            Some("https://example.com/cover.jpg")
        );
        assert_eq!(metadata.isbn.as_deref(), Some("9780306406157"));
    }

    #[test]
    fn main_work_kinds_and_exact_open_graph_types() {
        for (kind, category) in [
            ("Book", LinkCategory::Book),
            ("MusicAlbum", LinkCategory::Music),
            ("MusicRecording", LinkCategory::Music),
            ("MusicPlaylist", LinkCategory::Music),
            ("Movie", LinkCategory::Movie),
        ] {
            assert_eq!(
                parse(&format!(r#"{{"@type":"{kind}"}}"#)).unwrap().category,
                category
            );
        }
        for (kind, expected) in [
            ("book", Some(LinkCategory::Book)),
            ("music.album", Some(LinkCategory::Music)),
            ("music.song", Some(LinkCategory::Music)),
            ("music.playlist", Some(LinkCategory::Music)),
            ("video.movie", Some(LinkCategory::Movie)),
            ("video.tv_show", None),
            ("video.other", None),
            ("music", None),
        ] {
            assert_eq!(
                parse_link_metadata(
                    &format!("<meta property='og:type' content='{kind}'>"),
                    "https://example.com/work"
                )
                .map(|meta| meta.category),
                expected
            );
        }
    }

    #[test]
    fn scoped_microdata_and_rdfa_retain_entity_properties() {
        for html in ["<div itemscope itemtype='https://schema.org/Book'><meta itemprop='name' content='A book'><img itemprop='image' src='/book.jpg'></div>", "<div vocab='https://schema.org/' typeof='Thing MusicAlbum'><meta property='name' content='An album'><img property='image' src='/album.jpg'></div>"] {
            let metadata = parse_link_metadata(html, "https://example.com/work").unwrap();
            assert!(metadata.title.is_some());
            assert!(metadata.image_url.is_some());
        }
    }

    #[test]
    fn review_article_collection_and_tv_mentions_do_not_classify() {
        for kind in [
            "Review",
            "Article",
            "BlogPosting",
            "ItemList",
            "CollectionPage",
            "TVSeries",
            "TVEpisode",
        ] {
            let data = format!(
                r##"{{"@graph":[{{"@type":"{kind}","itemReviewed":{{"@id":"#work"}}}},{{"@type":"Movie","@id":"#work","url":"https://example.com/work"}}]}}"##
            );
            assert!(parse(&data).is_none(), "{kind}");
        }
        assert!(parse_link_metadata("<script type='application/ld+json'>{\"@type\":\"Article\"}</script><div itemscope itemtype='https://schema.org/Book'></div><meta property='og:type' content='book'>", "https://example.com/work").is_none());
        assert!(
            parse(r#"{"@type":"WebPage","url":"/other","mainEntity":{"@type":"Book"}}"#).is_none()
        );
    }

    #[test]
    fn scoped_product_isbn_validates_but_prices_never_establish_books() {
        assert_eq!(
            parse(r#"{"@type":"Product","isbn":"9780306406157"}"#)
                .unwrap()
                .category,
            LinkCategory::Book
        );
        for raw in [
            "9780306406158",
            "1234567890",
            "not a book",
            "978030640615💡",
        ] {
            assert!(parse(&format!(r#"{{"@type":"Product","isbn":"{raw}","offers":{{"price":20,"priceCurrency":"USD"}}}}"#)).is_none());
        }
        assert_eq!(
            normalize_isbn("0-8044-2957-X").as_deref(),
            Some("080442957X")
        );
    }

    #[test]
    fn route_fallback_is_narrow_and_works_without_html() {
        for (url, category) in [
            (
                "https://www.goodreads.com/book/show/12345-title",
                LinkCategory::Book,
            ),
            (
                "https://openlibrary.org/works/OL123W/Title",
                LinkCategory::Book,
            ),
            (
                "https://www.waterstones.com/book/title/author/9780306406157",
                LinkCategory::Book,
            ),
            (
                "https://open.spotify.com/album/1234567890123456789012",
                LinkCategory::Music,
            ),
            (
                "https://music.apple.com/us/album/title/123456",
                LinkCategory::Music,
            ),
            (
                "https://artist.bandcamp.com/track/title",
                LinkCategory::Music,
            ),
            ("https://letterboxd.com/film/a-film/", LinkCategory::Movie),
        ] {
            assert_eq!(
                parse_link_metadata("", url).unwrap().category,
                category,
                "{url}"
            );
        }
        for url in [
            "https://www.goodreads.com/book",
            "https://goodreads.com/review/show/1234",
            "https://goodreads.com.evil.example/book/show/1234",
            "https://openlibrary.org/search",
            "https://www.waterstones.com/book/title/author/9780306406158",
            "https://open.spotify.com/artist/1234567890123456789012",
            "https://letterboxd.com/person/films",
            "https://www.imdb.com/title/tt1234567/",
        ] {
            assert!(parse_link_metadata("", url).is_none(), "{url}");
        }
    }

    #[test]
    fn covers_reject_private_credentials_and_non_http_urls() {
        for image in [
            "",
            "http://127.0.0.1/a.jpg",
            "http://192.168.1.2/a.jpg",
            "http://[::1]/a.jpg",
            "http://localhost/a.jpg",
            "http://host.local/a.jpg",
            "https://user:pass@example.com/a.jpg",
            "data:image/png;base64,A",
            "file:///tmp/a.jpg",
        ] {
            assert!(
                parse(&format!(r#"{{"@type":"Book","image":"{image}"}}"#))
                    .unwrap()
                    .image_url
                    .is_none(),
                "{image}"
            );
        }
    }

    #[test]
    fn main_movie_survives_recommended_television_and_tv_main_stays_unclassified() {
        let movie =
            parse(r#"[{"@type":"Movie","url":"/work"},{"@type":"TVSeries","url":"/recommended"}]"#)
                .unwrap();
        assert_eq!(movie.category, LinkCategory::Movie);
        assert!(parse(r##"{"@graph":[{"@type":"WebPage","url":"/work","mainEntity":{"@id":"#tv"}},{"@id":"#tv","@type":"TVSeries","url":"/work"},{"@type":"Movie","url":"/recommended"}]}"##).is_none());
        assert_eq!(parse(r#"{"@type":"MusicAlbum","name":"Album","track":{"@type":"MusicRecording","name":"Track"}}"#).unwrap().title.as_deref(), Some("Album"));
    }

    #[test]
    fn og_cover_prefers_first_image_with_its_secure_variant_and_entity_cover_wins() {
        let og = "<meta property='og:type' content='book'><meta property='og:image' content='http://example.com/first.jpg'><meta property='og:image:secure_url' content='https://example.com/first.jpg'><meta property='og:image' content='https://example.com/second.jpg'><meta property='og:image:secure_url' content='https://example.com/second-secure.jpg'>";
        assert_eq!(
            parse_link_metadata(og, "https://example.com/work")
                .unwrap()
                .image_url
                .as_deref(),
            Some("https://example.com/first.jpg")
        );
        let entity = format!("{og}<script type='application/ld+json'>{{\"@type\":\"Book\",\"image\":\"/entity.jpg\"}}</script>");
        assert_eq!(
            parse_link_metadata(&entity, "https://example.com/work")
                .unwrap()
                .image_url
                .as_deref(),
            Some("https://example.com/entity.jpg")
        );
        assert_eq!(parse_link_metadata("<meta property='og:type' content='book'><meta property='og:image:url' content='/url.jpg'>", "https://example.com/work").unwrap().image_url.as_deref(), Some("https://example.com/url.jpg"));
    }

    #[test]
    fn bookshop_hex_route_and_scoped_gtin13_are_books_but_collection_is_not() {
        let url = "https://bookshop.org/p/books/the-secret-lives-of-color-kassia-st-clair/ebc5fbf5213226da";
        assert_eq!(
            parse_link_metadata("", url).unwrap().category,
            LinkCategory::Book
        );
        assert!(parse_link_metadata(
            "<script type='application/ld+json'>{\"@type\":\"CollectionPage\"}</script>",
            url
        )
        .is_none());
        assert_eq!(
            parse(r#"{"@type":"Product","gtin13":"9781840919356"}"#)
                .unwrap()
                .isbn
                .as_deref(),
            Some("9781840919356")
        );
        assert!(parse(r#"{"@type":"Product","gtin13":"1234567890128"}"#).is_none());
    }

    #[test]
    fn referenced_recommendations_and_bare_work_names_never_classify() {
        assert!(parse(r##"{"@graph":[{"@type":"WebPage","url":"/work","mentions":{"@id":"#book"}},{"@id":"#book","@type":"Book","name":"Related book"}]}"##).is_none());
        assert!(parse_link_metadata("<title>A book about music and movies</title><meta property='og:title' content='A book'><meta property='og:image' content='/cover.jpg'>", "https://example.com/work").is_none());
    }

    #[test]
    fn verified_isbn_cover_falls_back_after_entity_and_page_images() {
        let expected = "https://covers.openlibrary.org/b/isbn/9781840919356-L.jpg?default=false";
        let url = "https://www.waterstones.com/book/title/author/9781840919356";
        assert_eq!(
            parse_link_metadata("", url).unwrap().image_url.as_deref(),
            Some(expected)
        );
        assert_eq!(
            parse(r#"{"@type":"Book","isbn":"9781840919356"}"#)
                .unwrap()
                .image_url
                .as_deref(),
            Some(expected)
        );
        assert_eq!(
            parse(r#"{"@type":"Book","isbn":"9781840919356","image":"/source.jpg"}"#)
                .unwrap()
                .image_url
                .as_deref(),
            Some("https://example.com/source.jpg")
        );
    }

    #[test]
    fn explicit_review_tv_and_og_negative_context_prevents_route_fallback() {
        for kind in ["Review", "Article", "CollectionPage", "TVSeries"] {
            assert!(parse(&format!(r#"{{"@type":["WebPage","{kind}"],"url":"/work","mainEntity":{{"@type":"Book","image":"/cover.jpg"}}}}"#)).is_none(), "{kind}");
        }
        for (kind, url) in [
            ("article", "https://goodreads.com/book/show/1234"),
            ("video.tv_show", "https://letterboxd.com/film/a-show/"),
        ] {
            assert!(parse_link_metadata(&format!("<meta property='og:type' content='{kind}'><meta property='og:image' content='/cover.jpg'>"), url).is_none());
        }
    }

    #[test]
    fn same_provider_challenge_redirect_retains_only_original_route_evidence() {
        let original = "https://www.waterstones.com/book/title/author/9781840919356";
        let challenge = "<meta property='og:type' content='website'><meta property='og:title' content='Sign in'><meta property='og:image' content='/logo.png'>";
        let metadata =
            parse_fetched_page(challenge, original, "https://www.waterstones.com/challenge")
                .link
                .unwrap();
        assert_eq!(metadata.category, LinkCategory::Book);
        assert!(metadata.title.is_none());
        assert!(metadata
            .image_url
            .unwrap()
            .starts_with("https://covers.openlibrary.org/"));
        assert!(parse_fetched_page(
            "<meta property='og:type' content='article'>",
            original,
            "https://www.waterstones.com/review"
        )
        .link
        .is_none());
        assert!(
            parse_fetched_page(challenge, original, "https://other.example/challenge")
                .link
                .is_none()
        );
    }
    #[test]
    fn structured_depth_count_and_expansion_limits_reject_all_evidence() {
        let route = "https://goodreads.com/book/show/1234";
        let prefix = "<meta property='og:type' content='product'>";
        let nested = format!(
            "{prefix}{}<meta itemprop='name' content='Deep book'>{}",
            "<div itemscope itemtype='https://schema.org/Book' itemprop='workExample'>"
                .repeat(1600),
            "</div>".repeat(1600)
        );
        let deep_scopes = format!(
            "{prefix}{}{}",
            "<div itemscope itemtype='https://schema.org/Book' itemprop='workExample'>".repeat(40),
            "</div>".repeat(40)
        );
        let expanded = format!(
            "{prefix}{}<meta itemprop='name' content='Book'>{}",
            "<div itemscope itemtype='https://schema.org/Book' itemprop='a b c d e f g h'>"
                .repeat(16),
            "</div>".repeat(16)
        );
        let nodes = format!(
            "{prefix}<script type='application/ld+json'>[{}]</script>",
            std::iter::repeat_n(r#"{"@type":"Book"}"#, 1025)
                .collect::<Vec<_>>()
                .join(",")
        );
        let json_depth = format!(
            "{prefix}<script type='application/ld+json'>{}{}</script>",
            "[".repeat(160),
            "]".repeat(160)
        );
        for evidence in [nested, deep_scopes, expanded, nodes, json_depth] {
            assert!(
                parse_link_metadata(&evidence, route).is_none(),
                "a rejected structural family cannot fall back to provider/OG evidence"
            );
            assert!(
                crate::parse_product_metadata(&evidence, route).is_none(),
                "product detection uses the same atomic bounds"
            );
        }
        assert!(parse(r#"{"@type":"Book"}"#).is_some());
    }

    fn movie_catalog(id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"d":[{
            "id":id,"qid":"movie","q":"feature","l":" Catalog movie ",
            "i":{"imageUrl":"https://m.media-amazon.com/images/poster.jpg"}
        }]}))
        .unwrap()
    }

    #[test]
    fn imdb_catalog_requires_one_exact_movie_record() {
        let id = "tt0098554";
        let bytes = serde_json::to_vec(&serde_json::json!({"d":[
            {"id":"tt0944947","qid":"tvSeries","l":"Other title"},
            {"id":id,"qid":"movie","l":" Exact movie ","i":{"imageUrl":"https://m.media-amazon.com/images/poster.jpg"}}
        ]})).unwrap();
        let metadata = parse_imdb_catalog(&bytes, id).unwrap();
        assert_eq!(metadata.category, LinkCategory::Movie);
        assert_eq!(metadata.title.as_deref(), Some("Exact movie"));
        assert_eq!(
            metadata.image_url.as_deref(),
            Some("https://m.media-amazon.com/images/poster.jpg")
        );
        for response in [
            serde_json::json!({"d":[{"id":id,"qid":"tvSeries","q":"feature","l":"TV"}]}),
            serde_json::json!({"d":[{"id":id,"qid":"tvEpisode","l":"Episode"}]}),
            serde_json::json!({"d":[{"id":id,"q":"feature","l":"Unspecified"}]}),
            serde_json::json!({"d":[{"id":"tt9999999","qid":"movie","l":"Mismatch"}]}),
            serde_json::json!({"d":[{"id":id,"qid":"movie","l":"First"},{"id":id,"qid":"movie","l":"Second"}]}),
            serde_json::json!({"d":[]}),
        ] {
            assert!(parse_imdb_catalog(&serde_json::to_vec(&response).unwrap(), id).is_none());
        }
    }

    #[test]
    fn imdb_catalog_bounds_titles_payload_and_public_artwork() {
        let id = "tt0098554";
        for cover in [
            "https://127.0.0.1/poster.jpg",
            "https://user:pass@example.com/poster.jpg",
            "file:///poster.jpg",
            "/relative.jpg",
        ] {
            let bytes = serde_json::to_vec(&serde_json::json!({"d":[{"id":id,"qid":"movie","l":"Movie","i":{"imageUrl":cover}}]})).unwrap();
            let metadata = parse_imdb_catalog(&bytes, id).unwrap();
            assert!(metadata.image_url.is_none());
        }
        for title in [String::new(), " ".to_string(), "é".repeat(513)] {
            let bytes =
                serde_json::to_vec(&serde_json::json!({"d":[{"id":id,"qid":"movie","l":title}]}))
                    .unwrap();
            assert!(parse_imdb_catalog(&bytes, id).is_none());
        }
        assert!(parse_imdb_catalog(&vec![b' '; MAX_IMDB_CATALOG_BYTES + 1], id).is_none());
        assert!(parse_imdb_catalog(b"invalid JSON", id).is_none());
    }

    #[test]
    fn imdb_lookup_recognizes_only_exact_title_routes() {
        for url in [
            "https://imdb.com/title/tt0098554",
            "https://www.imdb.com/title/tt0098554/?ref_=tt_ov",
        ] {
            assert_eq!(imdb_title_id(url).as_deref(), Some("tt0098554"));
            assert!(
                parse_link_metadata("", url).is_none(),
                "live inspection never uses a catalog"
            );
        }
        for url in [
            "https://imdb.com/title/tt0098554/reviews",
            "https://imdb.com/title/tt0098554/episodes",
            "https://imdb.com/title/tt0098554//",
            "https://imdb.com/title/tt0098554?view=reviews",
            "https://imdb.com/title/tt0098554#reviews",
            "https://imdb.com/title/ttbadid",
            "https://imdb.com.evil.test/title/tt0098554",
            "https://user@imdb.com/title/tt0098554",
            "https://imdb.com/name/nm1234567",
        ] {
            assert!(imdb_title_id(url).is_none(), "{url}");
        }
    }

    #[test]
    fn imdb_url_fetch_falls_back_after_unavailable_or_unclassified_html() {
        let url = "https://imdb.com/title/tt0098554/";
        let endpoint = imdb_catalog_url("tt0098554");
        for unavailable in [true, false] {
            let mut calls = Vec::new();
            let metadata = fetch_page_metadata_with(url, |target, cap, deadline| {
                calls.push(target.to_string());
                if target == url {
                    assert_eq!(cap, MAX_PRODUCT_HTML_BYTES);
                    if unavailable {
                        anyhow::bail!("metadata HTTP 403");
                    }
                    return Ok((
                        b"<meta property='og:type' content='website'><title>IMDb</title>".to_vec(),
                        Some("text/html".to_string()),
                        url.to_string(),
                    ));
                }
                assert_eq!(target, endpoint);
                assert_eq!(cap, MAX_IMDB_CATALOG_BYTES);
                assert_eq!(deadline, IMDB_CATALOG_DEADLINE);
                Ok((
                    movie_catalog("tt0098554"),
                    Some("application/json; charset=utf-8".to_string()),
                    endpoint.clone(),
                ))
            })
            .unwrap();
            assert_eq!(calls, [url, &endpoint]);
            assert_eq!(
                metadata.link.unwrap().title.as_deref(),
                Some("Catalog movie")
            );
            assert!(metadata.product.is_none());
        }
    }

    #[test]
    fn imdb_page_evidence_and_atomic_budgets_veto_catalog_lookup() {
        let url = "https://imdb.com/title/tt0098554/";
        let mut evidence = Vec::new();
        for kind in [
            "TVSeries",
            "TVEpisode",
            "Article",
            "Review",
            "CollectionPage",
            "ItemList",
        ] {
            evidence.push(format!(
                "<script type='application/ld+json'>{{\"@type\":\"{kind}\"}}</script>"
            ));
        }
        evidence.push("<meta property='og:type' content='video.tv_show'>".to_string());
        evidence.push("<meta property='og:type' content='article'>".to_string());
        evidence.push(format!(
            "<script type='application/ld+json'>[{}]</script>",
            std::iter::repeat_n(r#"{"@type":"Movie"}"#, 1025)
                .collect::<Vec<_>>()
                .join(",")
        ));
        evidence.push("x".repeat(MAX_PRODUCT_HTML_BYTES + 1));
        for html in evidence {
            let mut calls = 0;
            let metadata = fetch_page_metadata_with(url, |target, _, _| {
                calls += 1;
                assert_eq!(
                    target, url,
                    "vetoed evidence must not trigger a catalog request"
                );
                Ok((
                    html.as_bytes().to_vec(),
                    Some("text/html".to_string()),
                    url.to_string(),
                ))
            })
            .unwrap();
            assert_eq!(calls, 1);
            assert!(metadata.link.is_none());
        }
        let mut calls = 0;
        assert!(fetch_page_metadata_with(url, |_, _, _| {
            calls += 1;
            anyhow::bail!("metadata exceeded size limit")
        })
        .unwrap()
        .link
        .is_none());
        assert_eq!(calls, 1);
    }

    #[test]
    fn imdb_main_metadata_wins_and_retains_product_snapshot() {
        let url = "https://imdb.com/title/tt0098554/";
        for html in [
            r#"<script type="application/ld+json">{"@type":"Movie","name":"Live title","image":"https://example.com/live.jpg"}</script>"#,
            r#"<script type="application/ld+json">{"@type":["Movie","Product"],"name":"Live title","offers":{"@type":"Offer","price":"12","priceCurrency":"EUR"}}</script>"#,
            r#"<script type="application/ld+json">{"@type":"Product","offers":{"@type":"Offer","price":"12","priceCurrency":"EUR"}}</script>"#,
        ] {
            let mut calls = 0;
            let metadata = fetch_page_metadata_with(url, |target, _, _| {
                calls += 1;
                assert_eq!(target, url);
                Ok((
                    html.as_bytes().to_vec(),
                    Some("text/html".to_string()),
                    url.to_string(),
                ))
            })
            .unwrap();
            assert_eq!(calls, 1);
            if html.contains("Live title") {
                assert_eq!(metadata.link.unwrap().title.as_deref(), Some("Live title"));
            }
            if html.contains("Product") {
                assert_eq!(metadata.product.unwrap().price.as_deref(), Some("12"));
            }
        }
    }

    #[test]
    fn bad_imdb_catalog_or_offline_fetch_keeps_an_ordinary_link() {
        let url = "https://imdb.com/title/tt0098554/";
        for case in 0..4 {
            let endpoint = imdb_catalog_url("tt0098554");
            let metadata = fetch_page_metadata_with(url, |target, _, _| {
                if target == url {
                    anyhow::bail!("page unavailable");
                }
                match case {
                    0 => anyhow::bail!("catalog unavailable"),
                    1 => Ok((
                        movie_catalog("tt0098554"),
                        Some("text/html".to_string()),
                        endpoint.clone(),
                    )),
                    2 => Ok((
                        movie_catalog("tt9999999"),
                        Some("application/json".to_string()),
                        endpoint.clone(),
                    )),
                    _ => Ok((
                        movie_catalog("tt0098554"),
                        Some("application/json".to_string()),
                        "https://example.com/catalog.json".to_string(),
                    )),
                }
            })
            .unwrap();
            assert!(metadata.link.is_none());
            assert!(metadata.product.is_none());
        }
    }
}
