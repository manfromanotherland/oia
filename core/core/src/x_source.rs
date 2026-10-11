// SPDX-License-Identifier: MIT

//! Pure classification and response parsing for public X posts.
//!
//! This module deliberately performs no network or filesystem I/O. Callers
//! classify a submitted URL, fetch [`syndication_url`] themselves, and pass the
//! response body to [`parse_syndication_payload`].

use serde::Deserialize;
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use url::Url;

mod note_text;

/// Prefer the best progressive MP4 no larger than this bitrate.
pub const PREFERRED_VIDEO_BITRATE: u64 = 2_000_000;

/// The stable identity recovered from a supported public post URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XPostReference {
    /// Lowercase X handle. X handles are case-insensitive.
    pub handle: String,
    /// Decimal post identifier, retained as a string to avoid integer limits.
    pub post_id: String,
    /// Alias- and query-free URL used as the reading's canonical source URL.
    pub canonical_url: String,
}

/// A provider-neutral attachment recovered from a public post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Video,
}

/// Remote attachment metadata. A later import stage owns downloading the URL
/// and replacing it with a reading-relative local asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAttachment {
    pub kind: AttachmentKind,
    /// Direct image or progressive MP4 URL.
    pub url: String,
    /// Video poster URL. Images do not need a separate poster.
    pub poster_url: Option<String>,
    pub content_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt: Option<String>,
    /// Selected video bitrate when the endpoint supplied one.
    pub bitrate: Option<u64>,
}

/// Provider-neutral fields needed to persist and render a social post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPost {
    pub source_id: String,
    /// Canonical post URL rebuilt from the payload's current author handle.
    pub canonical_url: String,
    pub text: String,
    pub display_name: String,
    /// Provider spelling is retained for display; URL classification lowers it
    /// only for canonical identity.
    pub handle: String,
    pub published_at: String,
    pub avatar_url: String,
    /// The endpoint's `mediaDetails` order is presentation order.
    pub attachments: Vec<ResolvedAttachment>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum XSourceError {
    #[error("the X syndication response was not valid JSON")]
    InvalidJson,
    #[error("the public X post is unavailable")]
    UnavailablePost,
    #[error("the public X response contains only truncated post text")]
    TruncatedPost,
    #[error("X Article wrappers are not supported by the social-post resolver")]
    UnsupportedArticle,
    #[error("the X syndication response is missing or has an invalid {0} field")]
    MalformedPayload(&'static str),
    #[error("the X video has no direct progressive MP4 variant")]
    MissingProgressiveVideo,
}

/// Recognise an exact public post route on the supported X and Twitter hosts.
///
/// Query parameters and fragments are share-time decoration and never enter
/// identity. An optional trailing slash and an exact photo/video viewer route
/// for one of the post's four attachment positions are accepted.
pub fn classify_x_post_url(raw_url: &str) -> Option<XPostReference> {
    let url = Url::parse(raw_url.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }

    match url.host_str()?.to_ascii_lowercase().as_str() {
        "x.com" | "www.x.com" | "twitter.com" | "www.twitter.com" => {}
        _ => return None,
    }

    let mut segments: Vec<_> = url.path_segments()?.collect();
    if segments.last() == Some(&"") {
        segments.pop();
    }
    let post_route = segments.len() == 3;
    let media_route = segments.len() == 5
        && matches!(segments[3], "photo" | "video")
        && matches!(segments[4], "1" | "2" | "3" | "4");
    if !(post_route || media_route) || segments[1] != "status" {
        return None;
    }

    let handle = segments[0];
    let post_id = segments[2];
    if !valid_handle(handle) || !decimal_identifier(post_id) {
        return None;
    }

    let handle = handle.to_ascii_lowercase();
    Some(XPostReference {
        canonical_url: format!("https://x.com/{handle}/status/{post_id}"),
        handle,
        post_id: post_id.to_owned(),
    })
}

/// Build the unauthenticated JSON endpoint used by X's public embed surface.
///
/// `token=0` keeps requests deterministic; the current public endpoint only
/// requires that the parameter be present. Fetching remains outside this pure
/// module so callers can enforce their own redirects, limits, and timeouts.
pub fn syndication_url(post: &XPostReference) -> String {
    format!(
        "https://cdn.syndication.twimg.com/tweet-result?id={}&lang=en&token=0",
        post.post_id
    )
}

/// Parse one public syndication response without performing any I/O.
pub fn parse_syndication_payload(payload: &str) -> Result<ResolvedPost, XSourceError> {
    let raw = parse_raw_syndication(payload)?;
    if raw.note_tweet.is_some() {
        return Err(XSourceError::TruncatedPost);
    }
    resolve_syndication_post(raw)
}

/// Resolve a long post only when its exact note result is present in the
/// official public post HTML. HTML is parsed as data; no script is executed.
pub fn parse_syndication_payload_with_note_html(
    payload: &str,
    html: &str,
) -> Result<ResolvedPost, XSourceError> {
    let raw = parse_raw_syndication(payload)?;
    let note = raw
        .note_tweet
        .as_ref()
        .ok_or(XSourceError::MalformedPayload("note_tweet"))?;
    let note_id = note_results_id(note)?.to_owned();
    let mut post = resolve_syndication_post(raw)?;
    post.text = note_text::full_note_text(html, &note_id, &post.source_id, &post.text)?;
    Ok(post)
}

pub(crate) struct NoteTweetRefinement {
    pub source_id: String,
    pub canonical_url: String,
}

pub(crate) fn note_tweet_refinement(
    payload: &str,
) -> Result<Option<NoteTweetRefinement>, XSourceError> {
    let raw = parse_raw_syndication(payload)?;
    let Some(note) = raw.note_tweet.as_ref() else {
        return Ok(None);
    };
    note_results_id(note)?;
    let post = resolve_syndication_post(raw)?;
    Ok(Some(NoteTweetRefinement {
        source_id: post.source_id,
        canonical_url: post.canonical_url,
    }))
}

fn note_results_id(note: &serde_json::Value) -> Result<&str, XSourceError> {
    note.get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| note_text::graphql_identifier(id, "NoteTweetResults").is_some())
        .ok_or(XSourceError::MalformedPayload("note_tweet.id"))
}

fn parse_raw_syndication(payload: &str) -> Result<RawTweet, XSourceError> {
    let raw: RawTweet = serde_json::from_str(payload).map_err(|_| XSourceError::InvalidJson)?;

    if raw.type_name.as_deref() != Some("Tweet") {
        return Err(XSourceError::UnavailablePost);
    }
    if raw.article.is_some() {
        return Err(XSourceError::UnsupportedArticle);
    }
    Ok(raw)
}

fn resolve_syndication_post(raw: RawTweet) -> Result<ResolvedPost, XSourceError> {
    let source_id = required_string(raw.id_str, "id_str")?;
    if !decimal_identifier(&source_id) {
        return Err(XSourceError::MalformedPayload("id_str"));
    }

    let raw_text = raw.text.ok_or(XSourceError::MalformedPayload("text"))?;
    let visible_text = match raw.display_text_range {
        Some([start, end]) => utf16_slice(&raw_text, start, end)
            .ok_or(XSourceError::MalformedPayload("display_text_range"))?,
        None => raw_text,
    };
    let text = decode_html_entities(&visible_text);

    let user = raw.user.ok_or(XSourceError::MalformedPayload("user"))?;
    let display_name = required_string(user.name, "user.name")?;
    let handle = required_string(user.screen_name, "user.screen_name")?;
    if !valid_handle(&handle) {
        return Err(XSourceError::MalformedPayload("user.screen_name"));
    }
    let canonical_url = format!(
        "https://x.com/{}/status/{source_id}",
        handle.to_ascii_lowercase()
    );
    let avatar_url =
        required_http_url(user.profile_image_url_https, "user.profile_image_url_https")?;

    let published_at = required_string(raw.created_at, "created_at")?;
    if OffsetDateTime::parse(&published_at, &Rfc3339).is_err() {
        return Err(XSourceError::MalformedPayload("created_at"));
    }

    let attachments = raw
        .media_details
        .unwrap_or_default()
        .into_iter()
        .map(resolve_attachment)
        .collect::<Result<Vec<_>, _>>()?;

    if text.is_empty() && attachments.is_empty() {
        return Err(XSourceError::MalformedPayload("text"));
    }

    Ok(ResolvedPost {
        source_id,
        canonical_url,
        text,
        display_name,
        handle,
        published_at,
        avatar_url,
        attachments,
    })
}

fn resolve_attachment(raw: RawMedia) -> Result<ResolvedAttachment, XSourceError> {
    let kind = raw
        .media_type
        .as_deref()
        .ok_or(XSourceError::MalformedPayload("mediaDetails.type"))?;
    let dimensions = raw.original_info.unwrap_or_default();
    let alt = raw.ext_alt_text.filter(|value| !value.is_empty());

    match kind {
        "photo" => Ok(ResolvedAttachment {
            kind: AttachmentKind::Image,
            url: required_http_url(raw.media_url_https, "mediaDetails.media_url_https")?,
            poster_url: None,
            content_type: None,
            width: dimensions.width,
            height: dimensions.height,
            alt,
            bitrate: None,
        }),
        "video" | "animated_gif" => {
            let poster_url =
                required_http_url(raw.media_url_https, "mediaDetails.media_url_https")?;
            let variants = raw
                .video_info
                .ok_or(XSourceError::MalformedPayload("mediaDetails.video_info"))?
                .variants;
            let selected = select_progressive_mp4(variants)?;

            Ok(ResolvedAttachment {
                kind: AttachmentKind::Video,
                url: selected.url,
                poster_url: Some(poster_url),
                content_type: Some("video/mp4".to_owned()),
                width: dimensions.width,
                height: dimensions.height,
                alt,
                bitrate: selected.bitrate,
            })
        }
        _ => Err(XSourceError::MalformedPayload("mediaDetails.type")),
    }
}

fn select_progressive_mp4(variants: Vec<RawVideoVariant>) -> Result<SelectedVideo, XSourceError> {
    let mut candidates = variants
        .into_iter()
        .filter(|variant| variant.content_type.as_deref() == Some("video/mp4"))
        .filter_map(|variant| {
            let url = valid_http_url(variant.url.as_deref()?)?;
            Some(SelectedVideo {
                url,
                bitrate: variant.bitrate,
            })
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return Err(XSourceError::MissingProgressiveVideo);
    }

    // URL is the final tie-breaker, making selection independent of payload
    // ordering. Rated variants sort ahead of variants without bitrate.
    candidates.sort_by(|left, right| match (left.bitrate, right.bitrate) {
        (Some(left_bitrate), Some(right_bitrate)) => left_bitrate
            .cmp(&right_bitrate)
            .then_with(|| left.url.cmp(&right.url)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.url.cmp(&right.url),
    });

    let preferred = candidates
        .iter()
        .filter(|candidate| {
            candidate
                .bitrate
                .is_some_and(|rate| rate <= PREFERRED_VIDEO_BITRATE)
        })
        .map(|candidate| candidate.bitrate.unwrap())
        .max();

    if let Some(preferred) = preferred {
        return candidates
            .into_iter()
            .filter(|candidate| candidate.bitrate == Some(preferred))
            .min_by(|left, right| left.url.cmp(&right.url))
            .ok_or(XSourceError::MissingProgressiveVideo);
    }

    // With no candidate beneath the cap, choose the lowest known bitrate. If
    // every MP4 omitted bitrate, lexical URL order is the deterministic fallback.
    Ok(candidates.remove(0))
}

fn required_string(value: Option<String>, field: &'static str) -> Result<String, XSourceError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(XSourceError::MalformedPayload(field))
}

fn required_http_url(value: Option<String>, field: &'static str) -> Result<String, XSourceError> {
    let value = value.ok_or(XSourceError::MalformedPayload(field))?;
    valid_http_url(&value).ok_or(XSourceError::MalformedPayload(field))
}

fn valid_http_url(value: &str) -> Option<String> {
    let parsed = Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return None;
    }
    Some(value.to_owned())
}

fn valid_handle(handle: &str) -> bool {
    (1..=15).contains(&handle.len())
        && handle
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn decimal_identifier(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn utf16_slice(value: &str, start: usize, end: usize) -> Option<String> {
    if start > end {
        return None;
    }
    let start = utf16_offset_to_byte(value, start)?;
    let end = utf16_offset_to_byte(value, end)?;
    Some(value[start..end].to_owned())
}

fn utf16_offset_to_byte(value: &str, target: usize) -> Option<usize> {
    let mut offset = 0;
    for (byte_index, character) in value.char_indices() {
        if offset == target {
            return Some(byte_index);
        }
        offset += character.len_utf16();
        if offset > target {
            return None;
        }
    }
    (offset == target).then_some(value.len())
}

fn decode_html_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut cursor = 0;

    while let Some(relative_ampersand) = value[cursor..].find('&') {
        let ampersand = cursor + relative_ampersand;
        decoded.push_str(&value[cursor..ampersand]);

        let Some(relative_semicolon) = value[ampersand + 1..].find(';') else {
            decoded.push_str(&value[ampersand..]);
            return decoded;
        };
        let semicolon = ampersand + 1 + relative_semicolon;
        let entity = &value[ampersand + 1..semicolon];

        if entity.len() <= 16 {
            if let Some(character) = decode_entity(entity) {
                decoded.push(character);
                cursor = semicolon + 1;
                continue;
            }
        }

        decoded.push('&');
        cursor = ampersand + 1;
    }

    decoded.push_str(&value[cursor..]);
    decoded
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{00a0}'),
        "ndash" => Some('\u{2013}'),
        "mdash" => Some('\u{2014}'),
        "hellip" => Some('\u{2026}'),
        "lsquo" => Some('\u{2018}'),
        "rsquo" => Some('\u{2019}'),
        "ldquo" => Some('\u{201c}'),
        "rdquo" => Some('\u{201d}'),
        numeric if numeric.starts_with("#x") || numeric.starts_with("#X") => {
            u32::from_str_radix(&numeric[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        numeric if numeric.starts_with('#') => {
            numeric[1..].parse::<u32>().ok().and_then(char::from_u32)
        }
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct RawTweet {
    #[serde(rename = "__typename")]
    type_name: Option<String>,
    id_str: Option<String>,
    text: Option<String>,
    display_text_range: Option<[usize; 2]>,
    note_tweet: Option<serde_json::Value>,
    article: Option<serde_json::Value>,
    created_at: Option<String>,
    user: Option<RawUser>,
    #[serde(rename = "mediaDetails")]
    media_details: Option<Vec<RawMedia>>,
}

#[derive(Debug, Deserialize)]
struct RawUser {
    name: Option<String>,
    screen_name: Option<String>,
    profile_image_url_https: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMedia {
    #[serde(rename = "type")]
    media_type: Option<String>,
    media_url_https: Option<String>,
    ext_alt_text: Option<String>,
    original_info: Option<RawDimensions>,
    video_info: Option<RawVideoInfo>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDimensions {
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RawVideoInfo {
    variants: Vec<RawVideoVariant>,
}

#[derive(Debug, Deserialize)]
struct RawVideoVariant {
    bitrate: Option<u64>,
    content_type: Option<String>,
    url: Option<String>,
}

#[derive(Debug)]
struct SelectedVideo {
    url: String,
    bitrate: Option<u64>,
}
