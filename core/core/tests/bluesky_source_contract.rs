// SPDX-License-Identifier: MIT

use oia_core::bluesky_source::{
    classify_bluesky_post_url, matches_bluesky_identity, parse_post_thread_payload,
    post_thread_url, AttachmentKind, BlueskySourceError,
};
use serde_json::{json, Value};

const VIDEO_FIXTURE: &str = include_str!("fixtures/bluesky-post-video.json");
const DID: &str = "did:plc:e5zplwgiznavptdxhx5n2zlp";
const CID: &str = "bafkreig5nf6elpzgtanaydqmaemj5sssyu4nacelrcosy6mz56wl75suzu";
const SOURCE_ID: &str = "at://did:plc:e5zplwgiznavptdxhx5n2zlp/app.bsky.feed.post/3muu5higixs2i";
const POST_URL: &str = "https://bsky.app/profile/montoulieu.dev/post/3muu5higixs2i";

#[test]
fn classifies_handle_and_did_post_routes_without_share_decoration() {
    for (url, actor) in [
        (POST_URL.to_owned(), "montoulieu.dev"),
        (format!("{POST_URL}/?share=1#video"), "montoulieu.dev"),
        (
            POST_URL.replace("montoulieu.dev", "Montoulieu.Dev"),
            "montoulieu.dev",
        ),
        (
            POST_URL.replace("https://bsky.app", "http://www.bsky.app"),
            "montoulieu.dev",
        ),
        (POST_URL.replace("montoulieu.dev", DID), DID),
        (
            POST_URL.replace("montoulieu.dev", &DID.replace(':', "%3A")),
            DID,
        ),
        (
            POST_URL.replace("montoulieu.dev", "did:web:montoulieu.dev"),
            "did:web:montoulieu.dev",
        ),
    ] {
        let reference = classify_bluesky_post_url(&url).unwrap();
        assert_eq!(reference.actor, actor);
        assert_eq!(reference.post_id, "3muu5higixs2i");
        assert_eq!(
            reference.source_id,
            format!("at://{actor}/app.bsky.feed.post/3muu5higixs2i")
        );
        assert_eq!(
            reference.canonical_url,
            format!("https://bsky.app/profile/{actor}/post/3muu5higixs2i")
        );
        let metadata_url = url::Url::parse(&post_thread_url(&reference)).unwrap();
        assert_eq!(metadata_url.host_str(), Some("public.api.bsky.app"));
        assert_eq!(metadata_url.path(), "/xrpc/app.bsky.feed.getPostThread");
        let query: Vec<_> = metadata_url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(
            query,
            [
                ("uri".into(), reference.source_id),
                ("depth".into(), "0".into()),
                ("parentHeight".into(), "0".into())
            ]
        );
    }
}

#[test]
fn rejects_profiles_feeds_lookalikes_and_invalid_actor_or_record_routes() {
    for url in [
        "https://example.com/profile/montoulieu.dev/post/3muu5higixs2i",
        "https://bsky.app.evil.example/profile/montoulieu.dev/post/3muu5higixs2i",
        "https://bsky.app/profile/montoulieu.dev",
        "https://bsky.app/profile/montoulieu.dev/feed/3muu5higixs2i",
        "https://bsky.app/profile/montoulieu.dev/post/3muu5higixs2i/video/1",
        "https://bsky.app/profile/montoulieu.dev/post/3muu5higixs2i/extra",
        "https://bsky.app/profile/montoulieu.dev/post/",
        "https://bsky.app/profile/localhost/post/3muu5higixs2i",
        "https://bsky.app/profile/127.0.0.1/post/3muu5higixs2i",
        "https://bsky.app/profile/-alice.dev/post/3muu5higixs2i",
        "https://bsky.app/profile/alice..dev/post/3muu5higixs2i",
        "https://bsky.app/profile/handle.invalid/post/3muu5higixs2i",
        "https://bsky.app/profile/did:plc:bad/post/3muu5higixs2i",
        "https://bsky.app/profile/did:web:localhost/post/3muu5higixs2i",
        "https://bsky.app/profile/did:web:alice.dev:path/post/3muu5higixs2i",
        "https://bsky.app/profile/did:key:abcdef/post/3muu5higixs2i",
        "https://bsky.app/profile/montoulieu.dev/post/%2Fetc",
        "https://bsky.app/profile/montoulieu.dev/post/%zz",
        "https://user:secret@bsky.app/profile/montoulieu.dev/post/3muu5higixs2i",
        "https://bsky.app:444/profile/montoulieu.dev/post/3muu5higixs2i",
        "ftp://bsky.app/profile/montoulieu.dev/post/3muu5higixs2i",
    ] {
        assert_eq!(classify_bluesky_post_url(url), None, "accepted {url}");
    }
}

#[test]
fn verifies_stable_identity_and_exact_requested_record() {
    let handle_uri = "at://montoulieu.dev/app.bsky.feed.post/3muu5higixs2i";
    assert!(matches_bluesky_identity(handle_uri, SOURCE_ID, POST_URL));
    assert!(matches_bluesky_identity(SOURCE_ID, SOURCE_ID, POST_URL));
    assert!(matches_bluesky_identity(
        SOURCE_ID,
        SOURCE_ID,
        &POST_URL.replace("montoulieu.dev", DID)
    ));
    assert!(matches_bluesky_identity(
        SOURCE_ID,
        SOURCE_ID,
        &POST_URL.replace("montoulieu.dev", "newhandle.dev")
    ));
    for (expected, actual, url) in [
        (
            handle_uri.to_owned(),
            SOURCE_ID.replace("3muu5higixs2i", "different"),
            POST_URL.to_owned(),
        ),
        (
            SOURCE_ID.to_owned(),
            SOURCE_ID.replace(DID, "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
            POST_URL.to_owned(),
        ),
        (
            handle_uri.to_owned(),
            handle_uri.to_owned(),
            POST_URL.to_owned(),
        ),
        (
            handle_uri.to_owned(),
            SOURCE_ID.to_owned(),
            POST_URL.replace("montoulieu.dev", "unrelated.dev"),
        ),
        (
            handle_uri.to_owned(),
            SOURCE_ID.to_owned(),
            format!("{POST_URL}?share=1"),
        ),
        (
            handle_uri.to_owned(),
            SOURCE_ID.to_owned(),
            POST_URL.replace("https://", "http://"),
        ),
        (
            handle_uri.to_owned(),
            SOURCE_ID.to_owned(),
            POST_URL.replace("3muu5higixs2i", "different"),
        ),
    ] {
        assert!(!matches_bluesky_identity(&expected, &actual, &url));
    }
}

#[test]
fn reported_post_keeps_complete_text_and_downloadable_original_video() {
    let post = parse_post_thread_payload(VIDEO_FIXTURE).unwrap();
    assert_eq!(post.source_id, SOURCE_ID);
    assert_eq!(post.canonical_url, POST_URL);
    assert_eq!(post.display_name, "Pieter Montoulieu");
    assert_eq!(post.handle, "montoulieu.dev");
    assert_eq!(post.published_at, "2026-09-06T13:55:32.482Z");
    assert_eq!(post.text, "Anyone remember the newsreader app, Paper?\n\nStarted prototyping an atproto take on it using @standard.site records");
    assert_eq!(post.attachments.len(), 1);
    let video = &post.attachments[0];
    assert_eq!(video.kind, AttachmentKind::Video);
    assert_eq!(video.width, Some(1258));
    assert_eq!(video.height, Some(2232));
    assert_eq!(video.content_type.as_deref(), Some("video/mp4"));
    assert_eq!(
        video.poster_url.as_deref(),
        Some(
            format!(
                "https://video.bsky.app/watch/{}/{CID}/thumbnail.jpg",
                DID.replace(':', "%3A")
            )
            .as_str()
        )
    );
    let blob_url = url::Url::parse(&video.url).unwrap();
    assert_eq!(blob_url.host_str(), Some("bsky.social"));
    assert_eq!(blob_url.path(), "/xrpc/com.atproto.sync.getBlob");
    assert_eq!(
        blob_url.query_pairs().collect::<Vec<_>>(),
        [("did".into(), DID.into()), ("cid".into(), CID.into())]
    );
    assert!(!video.url.ends_with(".m3u8"));
}

#[test]
fn preserves_all_four_images_in_source_order_and_accepts_missing_avatar() {
    let mut payload = fixture();
    add_images(&mut payload, 4);
    payload["thread"]["post"]["author"]
        .as_object_mut()
        .unwrap()
        .remove("avatar");
    payload["thread"]["post"]["author"]
        .as_object_mut()
        .unwrap()
        .remove("displayName");
    let post = parse(&payload).unwrap();
    assert_eq!(post.avatar_url, "");
    assert_eq!(post.display_name, "montoulieu.dev");
    assert_eq!(post.attachments.len(), 4);
    for (index, image) in post.attachments.iter().enumerate() {
        assert_eq!(image.kind, AttachmentKind::Image);
        assert_eq!(image.alt, Some(format!("Image {}", index + 1)));
        assert_eq!(image.width, Some(1000 + index as u32));
        assert_eq!(image.height, Some(800));
        assert_eq!(image.poster_url, None);
        assert!(image.url.ends_with(&format!("{}@jpeg", image_cid(index))));
    }
}

#[test]
fn record_with_media_keeps_parent_media_and_ignores_quoted_post_media() {
    let mut payload = fixture();
    add_images(&mut payload, 2);
    let post = &mut payload["thread"]["post"];
    post["record"]["embed"] = json!({
        "$type": "app.bsky.embed.recordWithMedia",
        "record": { "$type": "app.bsky.embed.record", "record": { "uri": "at://did:plc:aaaaaaaaaaaaaaaaaaaaaaaa/app.bsky.feed.post/quoted", "cid": CID } },
        "media": post["record"]["embed"].take()
    });
    post["embed"] = json!({
        "$type": "app.bsky.embed.recordWithMedia#view",
        "record": { "$type": "app.bsky.embed.record#view", "record": quote_view() },
        "media": post["embed"].take()
    });
    let resolved = parse(&payload).unwrap();
    assert_eq!(resolved.source_id, SOURCE_ID);
    assert_eq!(resolved.attachments.len(), 2);
    assert!(resolved.text.ends_with("Quoted post by Quoted author (@quoted.dev)\n\nQuoted text 🚀\n\nhttps://bsky.app/profile/quoted.dev/post/quoted"));
}

#[test]
fn quote_only_posts_preserve_verified_text_author_link_without_recursing() {
    let mut payload = fixture();
    add_quote(&mut payload);
    let post = parse(&payload).unwrap();
    assert!(post.attachments.is_empty());
    assert!(post.text.contains("Quoted text 🚀"));
    assert!(post
        .text
        .contains("https://bsky.app/profile/quoted.dev/post/quoted"));
    for (pointer, value) in [
        ("/thread/post/embed/record/cid", json!(image_cid(1))),
        ("/thread/post/embed/record/uri", json!(SOURCE_ID)),
        ("/thread/post/embed/record/author/did", json!(DID)),
        ("/thread/post/embed/record/value/text", Value::Null),
        (
            "/thread/post/embed/record/value/text",
            json!("x".repeat(16 * 1024 + 1)),
        ),
        (
            "/thread/post/embed/record/value/$type",
            json!("app.bsky.feed.generator"),
        ),
    ] {
        let mut payload = fixture();
        add_quote(&mut payload);
        *payload.pointer_mut(pointer).unwrap() = value;
        assert!(parse(&payload).is_err(), "accepted malformed {pointer}");
    }
}

#[test]
fn unavailable_quoted_records_preserve_the_parent_and_original_quote_link() {
    for (kind, flag) in [
        ("viewNotFound", "notFound"),
        ("viewBlocked", "blocked"),
        ("viewDetached", "detached"),
    ] {
        let mut payload = fixture();
        add_quote(&mut payload);
        payload["thread"]["post"]["embed"]["record"] = json!({
            "$type": format!("app.bsky.embed.record#{kind}"),
            "uri": "at://did:plc:aaaaaaaaaaaaaaaaaaaaaaaa/app.bsky.feed.post/quoted",
            flag: true
        });
        let post = parse(&payload).unwrap();
        assert!(post.text.ends_with("Quoted post unavailable\n\nhttps://bsky.app/profile/did:plc:aaaaaaaaaaaaaaaaaaaaaaaa/post/quoted"));
    }
}

#[test]
fn external_card_preserves_target_metadata_and_thumbnail() {
    let mut payload = fixture();
    let external = json!({"uri": "https://example.com/story", "title": "Story title", "description": "Story description"});
    payload["thread"]["post"]["record"]["embed"] = json!({
        "$type": "app.bsky.embed.external", "external": external
    });
    payload["thread"]["post"]["embed"] = json!({
        "$type": "app.bsky.embed.external#view", "external": external
    });
    assert!(parse(&payload)
        .unwrap()
        .text
        .ends_with("Story title\n\nStory description\n\nhttps://example.com/story"));
    assert!(parse(&payload).unwrap().attachments.is_empty());
    payload["thread"]["post"]["record"]["embed"]["external"]["thumb"] = blob(CID, "image/jpeg");
    payload["thread"]["post"]["embed"]["external"]["thumb"] = json!(format!(
        "https://cdn.bsky.app/img/feed_thumbnail/plain/{DID}/{CID}@jpeg"
    ));
    let post = parse(&payload).unwrap();
    assert_eq!(post.attachments.len(), 1);
    assert_eq!(post.attachments[0].kind, AttachmentKind::Image);
    assert_eq!(post.attachments[0].alt.as_deref(), Some("Story title"));
}

#[test]
fn text_only_posts_preserve_unicode_newlines_and_ignore_thread_replies() {
    let mut payload = fixture();
    let post = &mut payload["thread"]["post"];
    post.as_object_mut().unwrap().remove("embed");
    post["record"].as_object_mut().unwrap().remove("embed");
    post["record"]["text"] = json!("Full text 🚀 &\n\nNo truncation.");
    payload["thread"]["parent"] = json!({"post": {"record": {"text": "Parent"}}});
    payload["thread"]["replies"] = json!([{ "post": {"record": {"text": "Reply"}} }]);
    let resolved = parse(&payload).unwrap();
    assert_eq!(resolved.text, "Full text 🚀 &\n\nNo truncation.");
    assert!(resolved.attachments.is_empty());
}

#[test]
fn unavailable_posts_and_unsupported_embeds_are_explicit_failures() {
    assert_eq!(
        parse_post_thread_payload("not json"),
        Err(BlueskySourceError::InvalidJson)
    );
    for payload in [
        json!({"error": "NotFound"}),
        json!({"thread": {"$type": "app.bsky.feed.defs#notFoundPost", "uri": SOURCE_ID, "notFound": true}}),
        json!({"thread": {"$type": "app.bsky.feed.defs#blockedPost", "uri": SOURCE_ID, "blocked": true}}),
    ] {
        assert_eq!(parse(&payload), Err(BlueskySourceError::UnavailablePost));
    }
    for kind in ["app.bsky.embed.gallery", "unrecognized.embed"] {
        let mut payload = fixture();
        payload["thread"]["post"]["record"]["embed"] = json!({"$type": kind});
        payload["thread"]["post"]["embed"] = json!({"$type": format!("{kind}#view")});
        assert_eq!(parse(&payload), Err(BlueskySourceError::UnsupportedEmbed));
    }
}

fn quote_view() -> Value {
    json!({
        "$type": "app.bsky.embed.record#viewRecord",
        "uri": "at://did:plc:aaaaaaaaaaaaaaaaaaaaaaaa/app.bsky.feed.post/quoted",
        "cid": CID,
        "author": { "did": "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa", "handle": "quoted.dev", "displayName": "Quoted author" },
        "value": { "$type": "app.bsky.feed.post", "text": "Quoted text 🚀" },
        "indexedAt": "2026-09-05T13:55:32.482Z",
        "embeds": [{ "$type": "unsupported" }]
    })
}

fn add_quote(payload: &mut Value) {
    payload["thread"]["post"]["record"]["embed"] = json!({
        "$type": "app.bsky.embed.record",
        "record": { "uri": "at://did:plc:aaaaaaaaaaaaaaaaaaaaaaaa/app.bsky.feed.post/quoted", "cid": CID }
    });
    payload["thread"]["post"]["embed"] = json!({
        "$type": "app.bsky.embed.record#view",
        "record": quote_view()
    });
}

#[test]
fn incomplete_or_mismatched_required_metadata_and_video_are_rejected() {
    for (pointer, value) in [
        ("/thread/post/uri", json!("at://montoulieu.dev/app.bsky.feed.post/3muu5higixs2i")),
        ("/thread/post/author/did", json!("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa")),
        ("/thread/post/author/handle", json!("invalid")),
        ("/thread/post/author/avatar", json!("file:///secret")),
        ("/thread/post/record/$type", json!("wrong.record")),
        ("/thread/post/record/text", Value::Null),
        ("/thread/post/record/createdAt", json!("not a timestamp")),
        ("/thread/post/embed/$type", json!("app.bsky.embed.images#view")),
        ("/thread/post/embed/cid", json!(image_cid(1))),
        ("/thread/post/record/embed/video/mimeType", json!("image/jpeg")),
        ("/thread/post/record/embed/video/size", json!(0)),
        ("/thread/post/record/embed/video/ref/$link", json!("not-a-cid")),
        ("/thread/post/embed/playlist", json!(format!("https://video.bsky.app/watch/did%3Aplc%3Aaaaaaaaaaaaaaaaaaaaaaaaa/{CID}/playlist.m3u8"))),
        ("/thread/post/embed/playlist", json!(format!("https://evil.example/watch/{DID}/{CID}/playlist.m3u8"))),
        ("/thread/post/embed/thumbnail", json!(format!("https://video.bsky.app/watch/{DID}/{}/thumbnail.jpg", image_cid(1)))),
        ("/thread/post/embed/aspectRatio/width", json!(0)),
    ] {
        let mut payload = fixture();
        *payload.pointer_mut(pointer).unwrap() = value;
        assert!(parse(&payload).is_err(), "accepted malformed {pointer}");
    }
    let mut payload = fixture();
    payload["thread"]["post"]
        .as_object_mut()
        .unwrap()
        .remove("embed");
    assert!(parse(&payload).is_err());
}

#[test]
fn missing_or_mismatched_image_entries_never_silently_drop_media() {
    let mut payload = fixture();
    add_images(&mut payload, 4);
    payload["thread"]["post"]["embed"]["images"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(parse(&payload).is_err());
    for (pointer, value) in [
        ("/thread/post/embed/images/2/fullsize", Value::Null),
        (
            "/thread/post/embed/images/2/fullsize",
            json!(format!(
                "https://cdn.bsky.app/img/feed_fullsize/plain/{DID}/{CID}@jpeg"
            )),
        ),
        ("/thread/post/embed/images/2/alt", Value::Null),
        (
            "/thread/post/record/embed/images/2/image/mimeType",
            json!("video/mp4"),
        ),
    ] {
        let mut payload = fixture();
        add_images(&mut payload, 4);
        *payload.pointer_mut(pointer).unwrap() = value;
        assert!(parse(&payload).is_err(), "accepted malformed {pointer}");
    }
    let mut payload = fixture();
    add_images(&mut payload, 5);
    assert!(parse(&payload).is_err());
}

fn fixture() -> Value {
    serde_json::from_str(VIDEO_FIXTURE).unwrap()
}

fn parse(payload: &Value) -> Result<oia_core::bluesky_source::ResolvedPost, BlueskySourceError> {
    parse_post_thread_payload(&payload.to_string())
}

fn image_cid(index: usize) -> String {
    format!(
        "{}{}",
        &CID[..CID.len() - 1],
        char::from(b'a' + index as u8)
    )
}

fn blob(cid: &str, mime_type: &str) -> Value {
    json!({"$type": "blob", "ref": {"$link": cid}, "mimeType": mime_type, "size": 1000})
}

fn add_images(payload: &mut Value, count: usize) {
    let originals: Vec<_> = (0..count)
        .map(|index| {
            json!({
                "image": blob(&image_cid(index), "image/jpeg"),
                "alt": format!("Image {}", index + 1),
                "aspectRatio": {"width": 1000 + index, "height": 800}
            })
        })
        .collect();
    let views: Vec<_> = (0..count).map(|index| json!({
        "fullsize": format!("https://cdn.bsky.app/img/feed_fullsize/plain/{DID}/{}@jpeg", image_cid(index)),
        "thumb": format!("https://cdn.bsky.app/img/feed_thumbnail/plain/{DID}/{}@jpeg", image_cid(index)),
        "alt": format!("Image {}", index + 1),
        "aspectRatio": {"width": 1000 + index, "height": 800}
    })).collect();
    payload["thread"]["post"]["record"]["embed"] =
        json!({"$type": "app.bsky.embed.images", "images": originals});
    payload["thread"]["post"]["embed"] =
        json!({"$type": "app.bsky.embed.images#view", "images": views});
}
