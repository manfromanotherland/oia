// SPDX-License-Identifier: MIT

//! Pure classification and parsing for public Bluesky posts.
//!
//! Callers fetch [`post_thread_url`] with their own network policy. Images use
//! the AppView's full-size URLs; videos use the original MP4 blob rather than
//! treating a remote HLS playlist or poster as a saved video. The fixed
//! `bsky.social` blob gateway redirects to a Bluesky-hosted PDS; callers must
//! restrict those redirects. Self-hosted PDS endpoints are not discovered here.
//! Images, video, external cards, and quoted posts are supported. Quotes retain
//! their verified author, text, and post link; quoted media is not fetched
//! recursively. Other record kinds and unknown embed types fail explicitly.

use serde_json::Value;
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueskyPostReference {
    /// A normalized handle or a supported DID from the submitted URL.
    pub actor: String,
    pub post_id: String,
    /// May contain a handle until the AppView returns the stable author DID.
    pub source_id: String,
    pub canonical_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAttachment {
    pub kind: AttachmentKind,
    pub url: String,
    pub poster_url: Option<String>,
    pub content_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt: Option<String>,
    pub bitrate: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPost {
    /// The AppView's `at://DID/app.bsky.feed.post/rkey`, never a mutable handle.
    pub source_id: String,
    pub canonical_url: String,
    pub text: String,
    pub display_name: String,
    pub handle: String,
    pub published_at: String,
    /// An absent author avatar is valid and does not authorize a fetch.
    pub avatar_url: String,
    pub attachments: Vec<ResolvedAttachment>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum BlueskySourceError {
    #[error("the Bluesky response was not valid JSON")]
    InvalidJson,
    #[error("the public Bluesky post is unavailable")]
    UnavailablePost,
    #[error("the Bluesky response is missing or has an invalid {0} field")]
    MalformedPayload(&'static str),
    #[error("this Bluesky embed type is not supported by the social-post resolver")]
    UnsupportedEmbed,
}

/// Recognize exact post routes, excluding profiles, feeds, and arbitrary hosts.
pub fn classify_bluesky_post_url(raw_url: &str) -> Option<BlueskyPostReference> {
    let url = Url::parse(raw_url.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(url.host_str()?, "bsky.app" | "www.bsky.app")
    {
        return None;
    }
    let mut segments: Vec<_> = url.path_segments()?.collect();
    if segments.last() == Some(&"") {
        segments.pop();
    }
    if segments.len() != 4 || segments[0] != "profile" || segments[2] != "post" {
        return None;
    }
    let actor = decode_segment(segments[1])?;
    let post_id = decode_segment(segments[3])?;
    if !(valid_handle(&actor) || valid_did(&actor)) || !valid_record_key(&post_id) {
        return None;
    }
    let actor = if valid_handle(&actor) {
        actor.to_ascii_lowercase()
    } else {
        actor
    };
    Some(BlueskyPostReference {
        canonical_url: format!("https://bsky.app/profile/{actor}/post/{post_id}"),
        source_id: format!("at://{actor}/app.bsky.feed.post/{post_id}"),
        actor,
        post_id,
    })
}

/// Only the requested post is needed; parents and replies are excluded.
pub fn post_thread_url(post: &BlueskyPostReference) -> String {
    let mut url = Url::parse("https://public.api.bsky.app/xrpc/app.bsky.feed.getPostThread")
        .expect("static AppView URL");
    url.query_pairs_mut()
        .append_pair("uri", &post.source_id)
        .append_pair("depth", "0")
        .append_pair("parentHeight", "0");
    url.into()
}

/// Verify the returned stable DID identity against the submitted handle or DID.
/// A current handle may prove a handle reference; a DID reference must match
/// exactly. In both cases the record key and canonical post route must agree.
pub fn matches_bluesky_identity(
    expected_at_uri: &str,
    actual_did_at_uri: &str,
    canonical_url: &str,
) -> bool {
    let Some((expected_actor, expected_key)) = parse_at_post_uri(expected_at_uri) else {
        return false;
    };
    let Some((actual_did, actual_key)) = parse_at_post_uri(actual_did_at_uri) else {
        return false;
    };
    let Some(canonical) = classify_bluesky_post_url(canonical_url) else {
        return false;
    };
    if !valid_did(actual_did)
        || canonical.canonical_url != canonical_url
        || expected_key != actual_key
        || canonical.post_id != actual_key
    {
        return false;
    }
    if valid_did(expected_actor) {
        expected_actor == actual_did
            && (valid_handle(&canonical.actor) || canonical.actor == actual_did)
    } else {
        expected_actor.eq_ignore_ascii_case(&canonical.actor)
    }
}

/// Parse a complete public post without reading reply or parent content.
pub fn parse_post_thread_payload(payload: &str) -> Result<ResolvedPost, BlueskySourceError> {
    let raw: Value = serde_json::from_str(payload).map_err(|_| BlueskySourceError::InvalidJson)?;
    let thread = raw
        .get("thread")
        .ok_or(BlueskySourceError::UnavailablePost)?;
    if string(thread, "$type", "thread.$type")? != "app.bsky.feed.defs#threadViewPost" {
        return Err(BlueskySourceError::UnavailablePost);
    }
    let post = object(thread, "post", "thread.post")?;
    let source_id = string(post, "uri", "post.uri")?.to_owned();
    let (did, key) = parse_at_post_uri(&source_id)
        .filter(|(actor, _)| valid_did(actor))
        .ok_or(BlueskySourceError::MalformedPayload("post.uri"))?;
    let author = object(post, "author", "post.author")?;
    if string(author, "did", "author.did")? != did {
        return Err(BlueskySourceError::MalformedPayload("author.did"));
    }
    let handle = string(author, "handle", "author.handle")?;
    if !valid_handle(handle) {
        return Err(BlueskySourceError::MalformedPayload("author.handle"));
    }
    let handle = handle.to_ascii_lowercase();
    let display_name = optional_string(author, "displayName", "author.displayName")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&handle)
        .to_owned();
    let avatar_url = optional_string(author, "avatar", "author.avatar")?
        .filter(|value| !value.is_empty())
        .map(|url| http_url(url, "author.avatar"))
        .transpose()?
        .unwrap_or_default();
    let record = object(post, "record", "post.record")?;
    if string(record, "$type", "record.$type")? != "app.bsky.feed.post" {
        return Err(BlueskySourceError::MalformedPayload("record.$type"));
    }
    let mut text = string(record, "text", "record.text")?.to_owned();
    let published_at = string(record, "createdAt", "record.createdAt")?.to_owned();
    if OffsetDateTime::parse(&published_at, &Rfc3339).is_err() {
        return Err(BlueskySourceError::MalformedPayload("record.createdAt"));
    }
    let attachments = match (record.get("embed"), post.get("embed")) {
        (None, None) => vec![],
        (Some(record_embed), Some(view_embed)) => {
            resolve_embed(record_embed, view_embed, did, &mut text)?
        }
        _ => return Err(BlueskySourceError::MalformedPayload("post.embed")),
    };
    if text.is_empty() && attachments.is_empty() {
        return Err(BlueskySourceError::MalformedPayload("record.text"));
    }
    Ok(ResolvedPost {
        canonical_url: format!("https://bsky.app/profile/{handle}/post/{key}"),
        source_id,
        text,
        display_name,
        handle,
        published_at,
        avatar_url,
        attachments,
    })
}

fn resolve_embed(
    record: &Value,
    view: &Value,
    did: &str,
    text: &mut String,
) -> Result<Vec<ResolvedAttachment>, BlueskySourceError> {
    let kind = string(record, "$type", "record.embed.$type")?;
    let view_kind = string(view, "$type", "post.embed.$type")?;
    if view_kind != format!("{kind}#view") {
        return Err(BlueskySourceError::MalformedPayload("post.embed.$type"));
    }
    match kind {
        "app.bsky.embed.images" => resolve_images(record, view),
        "app.bsky.embed.video" => resolve_video(record, view, did).map(|video| vec![video]),
        "app.bsky.embed.external" => resolve_external(record, view, text),
        "app.bsky.embed.record" => {
            append_quoted_post(record, view, text)?;
            Ok(vec![])
        }
        "app.bsky.embed.recordWithMedia" => {
            // The parent post owns this media; never import media from the quoted post.
            append_quoted_post(
                object(record, "record", "record.embed.record")?,
                object(view, "record", "post.embed.record")?,
                text,
            )?;
            let record_media = object(record, "media", "record.embed.media")?;
            let view_media = object(view, "media", "post.embed.media")?;
            if string(record_media, "$type", "record.embed.media.$type")?
                == "app.bsky.embed.recordWithMedia"
            {
                return Err(BlueskySourceError::UnsupportedEmbed);
            }
            resolve_embed(record_media, view_media, did, text)
        }
        _ => Err(BlueskySourceError::UnsupportedEmbed),
    }
}

fn append_quoted_post(
    record: &Value,
    view: &Value,
    text: &mut String,
) -> Result<(), BlueskySourceError> {
    if string(record, "$type", "record.embed.record.$type")? != "app.bsky.embed.record"
        || string(view, "$type", "post.embed.record.$type")? != "app.bsky.embed.record#view"
    {
        return Err(BlueskySourceError::MalformedPayload("post.embed.record"));
    }
    let reference = object(record, "record", "record.embed.record")?;
    let expected_uri = string(reference, "uri", "record.embed.record.uri")?;
    let quoted = object(view, "record", "post.embed.record")?;
    let uri = string(quoted, "uri", "post.embed.record.uri")?;
    let (did, key) = parse_at_post_uri(uri)
        .filter(|(actor, _)| valid_did(actor))
        .ok_or(BlueskySourceError::MalformedPayload(
            "post.embed.record.uri",
        ))?;
    let kind = string(quoted, "$type", "post.embed.record.$type")?;
    let quote = match kind {
        "app.bsky.embed.record#viewRecord" => {
            if string(quoted, "cid", "post.embed.record.cid")?
                != string(reference, "cid", "record.embed.record.cid")?
            {
                return Err(BlueskySourceError::MalformedPayload(
                    "post.embed.record.cid",
                ));
            }
            let author = object(quoted, "author", "post.embed.record.author")?;
            if string(author, "did", "post.embed.record.author.did")? != did {
                return Err(BlueskySourceError::MalformedPayload(
                    "post.embed.record.author.did",
                ));
            }
            let handle = string(author, "handle", "post.embed.record.author.handle")?;
            if !valid_handle(handle) {
                return Err(BlueskySourceError::MalformedPayload(
                    "post.embed.record.author.handle",
                ));
            }
            let handle = handle.to_ascii_lowercase();
            let url = format!("https://bsky.app/profile/{handle}/post/{key}");
            if !matches_bluesky_identity(expected_uri, uri, &url) {
                return Err(BlueskySourceError::MalformedPayload(
                    "post.embed.record.uri",
                ));
            }
            let value = object(quoted, "value", "post.embed.record.value")?;
            if string(value, "$type", "post.embed.record.value.$type")? != "app.bsky.feed.post" {
                return Err(BlueskySourceError::UnsupportedEmbed);
            }
            let quoted_text = string(value, "text", "post.embed.record.value.text")?;
            let display_name = optional_string(
                author,
                "displayName",
                "post.embed.record.author.displayName",
            )?
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&handle);
            // This is one citation, not a recursively resolved thread.
            if quoted_text.len() > 16 * 1024 || display_name.len() > 1024 {
                return Err(BlueskySourceError::MalformedPayload(
                    "post.embed.record.value.text",
                ));
            }
            format!("Quoted post by {display_name} (@{handle})\n\n{quoted_text}\n\n{url}")
        }
        "app.bsky.embed.record#viewNotFound"
        | "app.bsky.embed.record#viewBlocked"
        | "app.bsky.embed.record#viewDetached" => {
            let flag = match kind {
                "app.bsky.embed.record#viewNotFound" => "notFound",
                "app.bsky.embed.record#viewBlocked" => "blocked",
                _ => "detached",
            };
            if expected_uri != uri || quoted.get(flag).and_then(Value::as_bool) != Some(true) {
                return Err(BlueskySourceError::MalformedPayload(
                    "post.embed.record.uri",
                ));
            }
            format!("Quoted post unavailable\n\nhttps://bsky.app/profile/{did}/post/{key}")
        }
        _ => return Err(BlueskySourceError::UnsupportedEmbed),
    };
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(&quote);
    Ok(())
}

fn resolve_images(
    record: &Value,
    view: &Value,
) -> Result<Vec<ResolvedAttachment>, BlueskySourceError> {
    let originals = array(record, "images", "record.embed.images")?;
    let images = array(view, "images", "post.embed.images")?;
    if !(1..=4).contains(&images.len()) || images.len() != originals.len() {
        return Err(BlueskySourceError::MalformedPayload("post.embed.images"));
    }
    originals
        .iter()
        .zip(images)
        .map(|(original, image)| {
            let blob = object(original, "image", "record.embed.images.image")?;
            let cid = blob_cid(blob, "record.embed.images.image")?;
            let mime = string(blob, "mimeType", "record.embed.images.image.mimeType")?;
            if !mime.starts_with("image/") {
                return Err(BlueskySourceError::MalformedPayload(
                    "record.embed.images.image.mimeType",
                ));
            }
            let url = http_url(
                string(image, "fullsize", "post.embed.images.fullsize")?,
                "post.embed.images.fullsize",
            )?;
            if !cdn_blob_matches(&url, cid) {
                return Err(BlueskySourceError::MalformedPayload(
                    "post.embed.images.fullsize",
                ));
            }
            let (width, height) = dimensions(image)?;
            Ok(ResolvedAttachment {
                kind: AttachmentKind::Image,
                url,
                poster_url: None,
                content_type: None,
                width,
                height,
                alt: Some(string(image, "alt", "post.embed.images.alt")?.to_owned())
                    .filter(|value| !value.is_empty()),
                bitrate: None,
            })
        })
        .collect()
}

fn resolve_video(
    record: &Value,
    view: &Value,
    did: &str,
) -> Result<ResolvedAttachment, BlueskySourceError> {
    let blob = object(record, "video", "record.embed.video")?;
    let cid = blob_cid(blob, "record.embed.video")?;
    if string(blob, "mimeType", "record.embed.video.mimeType")? != "video/mp4"
        || string(view, "cid", "post.embed.cid")? != cid
    {
        return Err(BlueskySourceError::MalformedPayload("record.embed.video"));
    }
    let playlist = http_url(
        string(view, "playlist", "post.embed.playlist")?,
        "post.embed.playlist",
    )?;
    if !video_cdn_matches(&playlist, did, cid, "playlist.m3u8") {
        return Err(BlueskySourceError::MalformedPayload("post.embed.playlist"));
    }
    let poster_url = optional_string(view, "thumbnail", "post.embed.thumbnail")?
        .map(|url| http_url(url, "post.embed.thumbnail"))
        .transpose()?;
    if poster_url
        .as_ref()
        .is_some_and(|url| !video_cdn_matches(url, did, cid, "thumbnail.jpg"))
    {
        return Err(BlueskySourceError::MalformedPayload("post.embed.thumbnail"));
    }
    let mut url = Url::parse("https://bsky.social/xrpc/com.atproto.sync.getBlob")
        .expect("static blob gateway URL");
    url.query_pairs_mut()
        .append_pair("did", did)
        .append_pair("cid", cid);
    let (width, height) = dimensions(view)?;
    Ok(ResolvedAttachment {
        kind: AttachmentKind::Video,
        url: url.into(),
        poster_url,
        content_type: Some("video/mp4".into()),
        width,
        height,
        alt: optional_string(view, "alt", "post.embed.alt")?
            .or(optional_string(record, "alt", "record.embed.alt")?)
            .map(str::to_owned)
            .filter(|value| !value.is_empty()),
        bitrate: None,
    })
}

fn resolve_external(
    record: &Value,
    view: &Value,
    text: &mut String,
) -> Result<Vec<ResolvedAttachment>, BlueskySourceError> {
    let original = object(record, "external", "record.embed.external")?;
    let external = object(view, "external", "post.embed.external")?;
    let uri = string(external, "uri", "post.embed.external.uri")?;
    http_url(uri, "post.embed.external.uri")?;
    if uri != string(original, "uri", "record.embed.external.uri")? {
        return Err(BlueskySourceError::MalformedPayload(
            "post.embed.external.uri",
        ));
    }
    let title = string(external, "title", "post.embed.external.title")?;
    let description = string(external, "description", "post.embed.external.description")?;
    for value in [title, description, uri]
        .into_iter()
        .filter(|value| !value.is_empty())
    {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(value);
    }
    match (
        original.get("thumb"),
        optional_string(external, "thumb", "post.embed.external.thumb")?,
    ) {
        (None, None) => Ok(vec![]),
        (Some(blob), Some(thumb)) => {
            let cid = blob_cid(blob, "record.embed.external.thumb")?;
            let url = http_url(thumb, "post.embed.external.thumb")?;
            if !cdn_blob_matches(&url, cid) {
                return Err(BlueskySourceError::MalformedPayload(
                    "post.embed.external.thumb",
                ));
            }
            Ok(vec![ResolvedAttachment {
                kind: AttachmentKind::Image,
                url,
                poster_url: None,
                content_type: None,
                width: None,
                height: None,
                alt: Some(title.to_owned()).filter(|value| !value.is_empty()),
                bitrate: None,
            }])
        }
        _ => Err(BlueskySourceError::MalformedPayload(
            "post.embed.external.thumb",
        )),
    }
}

fn blob_cid<'a>(blob: &'a Value, field: &'static str) -> Result<&'a str, BlueskySourceError> {
    if string(blob, "$type", field)? != "blob"
        || blob
            .get("size")
            .and_then(Value::as_u64)
            .is_none_or(|size| size == 0)
    {
        return Err(BlueskySourceError::MalformedPayload(field));
    }
    let cid = string(object(blob, "ref", field)?, "$link", field)?;
    if !(20..=128).contains(&cid.len())
        || !cid.starts_with('b')
        || !cid
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || matches!(byte, b'2'..=b'7'))
    {
        return Err(BlueskySourceError::MalformedPayload(field));
    }
    Ok(cid)
}

fn cdn_blob_matches(url: &str, cid: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    url.host_str() == Some("cdn.bsky.app")
        && url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|file| {
                file == cid
                    || file
                        .strip_prefix(cid)
                        .is_some_and(|suffix| suffix.starts_with('@'))
            })
}

fn video_cdn_matches(url: &str, did: &str, cid: &str, filename: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    if url.host_str() != Some("video.bsky.app") {
        return false;
    }
    let segments: Vec<_> = url.path_segments().into_iter().flatten().collect();
    segments.len() == 4
        && segments[0] == "watch"
        && decode_segment(segments[1]).as_deref() == Some(did)
        && segments[2] == cid
        && segments[3] == filename
}

fn dimensions(value: &Value) -> Result<(Option<u32>, Option<u32>), BlueskySourceError> {
    let Some(ratio) = value.get("aspectRatio") else {
        return Ok((None, None));
    };
    let dimension = |key| {
        ratio
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|number| u32::try_from(number).ok())
            .filter(|number| *number > 0)
            .ok_or(BlueskySourceError::MalformedPayload("embed.aspectRatio"))
    };
    Ok((Some(dimension("width")?), Some(dimension("height")?)))
}

fn string<'a>(
    value: &'a Value,
    key: &str,
    field: &'static str,
) -> Result<&'a str, BlueskySourceError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(BlueskySourceError::MalformedPayload(field))
}

fn optional_string<'a>(
    value: &'a Value,
    key: &str,
    field: &'static str,
) -> Result<Option<&'a str>, BlueskySourceError> {
    value
        .get(key)
        .map(|value| {
            value
                .as_str()
                .ok_or(BlueskySourceError::MalformedPayload(field))
        })
        .transpose()
}

fn object<'a>(
    value: &'a Value,
    key: &str,
    field: &'static str,
) -> Result<&'a Value, BlueskySourceError> {
    value
        .get(key)
        .filter(|value| value.is_object())
        .ok_or(BlueskySourceError::MalformedPayload(field))
}

fn array<'a>(
    value: &'a Value,
    key: &str,
    field: &'static str,
) -> Result<&'a Vec<Value>, BlueskySourceError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or(BlueskySourceError::MalformedPayload(field))
}

fn http_url(raw: &str, field: &'static str) -> Result<String, BlueskySourceError> {
    let url = Url::parse(raw).map_err(|_| BlueskySourceError::MalformedPayload(field))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(BlueskySourceError::MalformedPayload(field));
    }
    Ok(url.into())
}

fn parse_at_post_uri(raw: &str) -> Option<(&str, &str)> {
    let route = raw.strip_prefix("at://")?;
    let (actor, key) = route.split_once("/app.bsky.feed.post/")?;
    ((valid_handle(actor) || valid_did(actor)) && valid_record_key(key)).then_some((actor, key))
}

fn valid_did(value: &str) -> bool {
    if let Some(plc) = value.strip_prefix("did:plc:") {
        plc.len() == 24
            && plc
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || matches!(byte, b'2'..=b'7'))
    } else {
        value.strip_prefix("did:web:").is_some_and(valid_handle)
    }
}

fn valid_handle(value: &str) -> bool {
    if value.len() > 253 {
        return false;
    }
    let segments: Vec<_> = value.split('.').collect();
    segments.len() >= 2
        && segments.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        && segments
            .last()
            .is_some_and(|label| label.as_bytes()[0].is_ascii_alphabetic())
        && !matches!(
            segments.last().copied(),
            Some(
                "alt"
                    | "arpa"
                    | "example"
                    | "internal"
                    | "invalid"
                    | "local"
                    | "localhost"
                    | "onion"
                    | "test"
            )
        )
}

fn valid_record_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value != "."
        && value != ".."
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'~' | b':')
        })
}

fn decode_segment(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = (bytes.get(index + 1).copied()? as char).to_digit(16)?;
            let low = (bytes.get(index + 2).copied()? as char).to_digit(16)?;
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}
