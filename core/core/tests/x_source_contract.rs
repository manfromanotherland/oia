// SPDX-License-Identifier: MIT

use oia_core::x_source::{
    classify_x_post_url, parse_syndication_payload, parse_syndication_payload_with_note_html,
    syndication_url, AttachmentKind, XSourceError,
};

const PHOTO_FIXTURE: &str = include_str!("fixtures/x-syndication-photo.json");
const VIDEO_FIXTURE: &str = include_str!("fixtures/x-syndication-video.json");
const NOTE_TWEET_FIXTURE: &str = include_str!("fixtures/x-syndication-note-tweet.json");
const ARTICLE_FIXTURE: &str = include_str!("fixtures/x-syndication-article.json");
const FULL_NOTE_FIXTURE: &str = include_str!("fixtures/x-syndication-full-note.json");
const NOTE_PAGE_FIXTURE: &str = include_str!("fixtures/x-note-page.html");
const NOTE_TEXT: &str = include_str!("fixtures/x-note-full-text.txt");
const SHORT_NOTE_PHOTOS_FIXTURE: &str =
    include_str!("fixtures/x-syndication-short-note-photos.json");
const SHORT_NOTE_PAGE_FIXTURE: &str = include_str!("fixtures/x-short-note-page.html");

#[test]
fn official_note_page_accepts_already_complete_text_and_every_photo() {
    let post = parse_syndication_payload_with_note_html(
        SHORT_NOTE_PHOTOS_FIXTURE,
        SHORT_NOTE_PAGE_FIXTURE,
    )
    .unwrap();
    assert_eq!(post.source_id, "1726433216511316241");
    assert_eq!(
        post.text,
        "\"Aesthetic pleasure is not to be taken lightly; it's very important for the spirit and therefore the health.\" \n\n— Tadao Ando"
    );
    assert_eq!(post.attachments.len(), 4);
    for (attachment, (filename, width, height)) in post.attachments.iter().zip([
        ("F_WDPxyW0AAHMGv.jpg", 810, 1214),
        ("F_WEqL_XQAEOS6o.jpg", 1280, 1600),
        ("F_WFJuVXEAEvVZb.jpg", 1280, 1603),
        ("F_WFWOoWEAAtUKR.jpg", 1250, 834),
    ]) {
        assert_eq!(attachment.kind, AttachmentKind::Image);
        assert_eq!(
            attachment.url,
            format!("https://pbs.twimg.com/media/{filename}")
        );
        assert_eq!(attachment.width, Some(width));
        assert_eq!(attachment.height, Some(height));
    }
}

#[test]
fn official_note_page_recovers_complete_text_with_verified_syndication_media() {
    let post =
        parse_syndication_payload_with_note_html(FULL_NOTE_FIXTURE, NOTE_PAGE_FIXTURE).unwrap();
    let ordinary = parse_syndication_payload(VIDEO_FIXTURE).unwrap();
    assert_eq!(post.text, NOTE_TEXT);
    assert_eq!(post.source_id, ordinary.source_id);
    assert_eq!(post.canonical_url, ordinary.canonical_url);
    assert_eq!(post.handle, ordinary.handle);
    assert_eq!(post.published_at, ordinary.published_at);
    assert_eq!(post.attachments, ordinary.attachments);
    assert_eq!(post.avatar_url, ordinary.avatar_url);
}

#[test]
fn note_pages_require_matching_owned_unambiguous_complete_results() {
    let text_literal = serde_json::to_string(NOTE_TEXT).unwrap();
    let preview = "This long post 🚀 & starts here.";
    for html in [
        NOTE_PAGE_FIXTURE.replace("__typename:\"Tweet\"", "__typename:\"User\""),
        NOTE_PAGE_FIXTURE.replace("VHdlZXQ6MjEwMjUwNTc0MzI3ODgyOTg0MA==", "VHdlZXQ6NDI="),
        NOTE_PAGE_FIXTURE.replace("rest_id:\"2102505743278829840\"", "rest_id:\"42\""),
        NOTE_PAGE_FIXTURE.replace("rest_id:\"2102505743278829840\"", "rest_id:42"),
        NOTE_PAGE_FIXTURE.replace("__typename:\"NoteTweet\"", "__typename:\"Tweet\""),
        NOTE_PAGE_FIXTURE.replace("Tm90ZVR3ZWV0OjIxMDI1MDU3NDMyNzg4Mjk4NDE=", "Tm90ZVR3ZWV0OjQy"),
        NOTE_PAGE_FIXTURE.replace("rest_id:\"2102505743278829841\"", "rest_id:\"42\""),
        NOTE_PAGE_FIXTURE.replacen(&text_literal, &serde_json::to_string(&(NOTE_TEXT.to_owned()+" Conflicting result.")).unwrap(), 1),
        NOTE_PAGE_FIXTURE.replace(&text_literal, "\"Different unrelated text.\""),
        NOTE_PAGE_FIXTURE.replace(&text_literal, &serde_json::to_string(preview.trim_end_matches('.')).unwrap()),
        NOTE_PAGE_FIXTURE.replace(&text_literal, &serde_json::to_string(&(NOTE_TEXT.to_owned()+"…")).unwrap()),
        NOTE_PAGE_FIXTURE.replace(&text_literal, &serde_json::to_string(&(NOTE_TEXT.to_owned()+"...")).unwrap()),
        NOTE_PAGE_FIXTURE.replace(&text_literal, "\"invalid\\q string\""),
        NOTE_PAGE_FIXTURE.replace(&text_literal, &format!("{text_literal},text:{text_literal}")),
        "<script>var data={note_tweet_results:{id:\"unrelated\",result:{text:\"Incomplete\"}}};</script>".into(),
    ] {
        assert!(parse_syndication_payload_with_note_html(FULL_NOTE_FIXTURE, &html).is_err());
    }
}

#[test]
fn serialized_note_text_inside_comments_or_strings_cannot_authorize_a_save() {
    let source = NOTE_PAGE_FIXTURE
        .split("<script>")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    for html in [
        format!("<script>/*{source}*/</script>"),
        format!(
            "<script>var inert={};</script>",
            serde_json::to_string(source).unwrap()
        ),
        format!(
            "<script>{}</script>",
            source
                .chars()
                .take(source.chars().count() / 2)
                .collect::<String>()
        ),
    ] {
        assert!(parse_syndication_payload_with_note_html(FULL_NOTE_FIXTURE, &html).is_err());
    }
}

#[test]
fn a_matching_note_result_without_its_owning_tweet_is_rejected() {
    let metadata: serde_json::Value = serde_json::from_str(FULL_NOTE_FIXTURE).unwrap();
    let html = format!(
        "<script>var note={{note_tweet_results:{{id:{},result:{{__typename:\"NoteTweet\",id:\"Tm90ZVR3ZWV0OjIxMDI1MDU3NDMyNzg4Mjk4NDE=\",text:{}}}}}}};</script>",
        metadata["note_tweet"]["id"],
        serde_json::to_string(NOTE_TEXT).unwrap()
    );
    assert!(parse_syndication_payload_with_note_html(FULL_NOTE_FIXTURE, &html).is_err());
}

#[test]
fn public_note_pages_are_bounded_by_bytes_and_object_complexity() {
    let excessive_objects = "{},".repeat(16_385);
    let excessive_depth = "{".repeat(65) + &"}".repeat(65);
    for html in [
        " ".repeat(2 * 1024 * 1024) + NOTE_PAGE_FIXTURE,
        format!(
            "<script>var note_tweet_results=[{excessive_objects}];</script>{NOTE_PAGE_FIXTURE}"
        ),
        format!("<script>var note_tweet_results={excessive_depth};</script>{NOTE_PAGE_FIXTURE}"),
    ] {
        assert!(parse_syndication_payload_with_note_html(FULL_NOTE_FIXTURE, &html).is_err());
    }
}

#[test]
fn classifies_supported_post_urls_and_canonicalizes_aliases() {
    let cases = [
        (
            "https://x.com/BenSpringwater/status/2102505743278829840?s=12&t=share#ignored",
            "benspringwater",
        ),
        (
            "https://www.x.com/benspringwater/status/2102505743278829840/",
            "benspringwater",
        ),
        (
            "http://twitter.com/BenSpringwater/status/2102505743278829840?ref_src=twsrc%5Etfw",
            "benspringwater",
        ),
        (
            "https://www.twitter.com/benspringwater/status/2102505743278829840",
            "benspringwater",
        ),
        (
            "https://twitter.com/BenSpringwater/status/2102505743278829840/photo/2?ssr=true",
            "benspringwater",
        ),
        (
            "https://x.com/BenSpringwater/status/2102505743278829840/video/4/",
            "benspringwater",
        ),
    ];

    for (raw, expected_handle) in cases {
        let post = classify_x_post_url(raw).expect(raw);
        assert_eq!(post.handle, expected_handle);
        assert_eq!(post.post_id, "2102505743278829840");
        assert_eq!(
            post.canonical_url,
            "https://x.com/benspringwater/status/2102505743278829840"
        );
        assert_eq!(
            syndication_url(&post),
            "https://cdn.syndication.twimg.com/tweet-result?id=2102505743278829840&lang=en&token=0"
        );
    }
}

#[test]
fn rejects_non_post_or_non_public_x_urls() {
    for raw in [
        "https://example.com/ed/status/123",
        "https://mobile.x.com/ed/status/123",
        "https://x.com/ed",
        "https://x.com/ed/status/not-a-number",
        "https://x.com/ed/status/123/photo/0",
        "https://x.com/ed/status/123/photo/5",
        "https://x.com/ed/status/123/photo/01",
        "https://x.com/ed/status/123/video/not-a-number",
        "https://x.com/ed/status/123/photo/1/extra",
        "https://x.com/ed/status/123/analytics",
        "https://x.com/i/web/status/123",
        "https://x.com/too-long-for-an-x-handle/status/123",
        "https://user:password@x.com/ed/status/123",
        "ftp://x.com/ed/status/123",
    ] {
        assert_eq!(classify_x_post_url(raw), None, "accepted {raw}");
    }
}

#[test]
fn parses_text_and_photos_in_source_order() {
    let post = parse_syndication_payload(PHOTO_FIXTURE).unwrap();

    assert_eq!(post.source_id, "1760744749433045051");
    assert_eq!(
        post.canonical_url,
        "https://x.com/nasawebb/status/1760744749433045051"
    );
    assert_eq!(post.text, "A & B");
    assert_eq!(post.display_name, "NASA Webb Telescope");
    assert_eq!(post.handle, "NASAWebb");
    assert_eq!(post.published_at, "2024-02-22T19:13:55.000Z");
    assert_eq!(
        post.avatar_url,
        "https://pbs.twimg.com/profile_images/1767989888916299776/hFYvpxZM_normal.jpg"
    );
    assert_eq!(post.attachments.len(), 2);
    assert_eq!(post.attachments[0].kind, AttachmentKind::Image);
    assert_eq!(
        post.attachments[0].url,
        "https://pbs.twimg.com/media/first.jpg"
    );
    assert_eq!(post.attachments[0].poster_url, None);
    assert_eq!(post.attachments[0].width, Some(1402));
    assert_eq!(post.attachments[0].height, Some(834));
    assert_eq!(post.attachments[0].alt.as_deref(), Some("The first image"));
    assert_eq!(post.attachments[1].kind, AttachmentKind::Image);
    assert_eq!(
        post.attachments[1].url,
        "https://pbs.twimg.com/media/second.png"
    );
}

#[test]
fn parses_video_and_selects_the_highest_progressive_mp4_under_the_cap() {
    let post = parse_syndication_payload(VIDEO_FIXTURE).unwrap();

    assert_eq!(post.text, "Motion \"study\"");
    assert_eq!(post.attachments.len(), 1);
    let video = &post.attachments[0];
    assert_eq!(video.kind, AttachmentKind::Video);
    assert_eq!(video.url, "https://video.twimg.com/amplify_video/720.mp4");
    assert_eq!(video.bitrate, Some(1_280_000));
    assert_eq!(video.content_type.as_deref(), Some("video/mp4"));
    assert_eq!(
        video.poster_url.as_deref(),
        Some("https://pbs.twimg.com/amplify_video_thumb/2102504201188499456/img/poster.jpg")
    );
}

#[test]
fn selects_the_lowest_mp4_when_every_variant_exceeds_the_cap() {
    let mut payload: serde_json::Value = serde_json::from_str(VIDEO_FIXTURE).unwrap();
    payload["mediaDetails"][0]["video_info"]["variants"] = serde_json::json!([
        {
            "bitrate": 6_000_000,
            "content_type": "video/mp4",
            "url": "https://video.twimg.com/high.mp4"
        },
        {
            "bitrate": 3_000_000,
            "content_type": "video/mp4",
            "url": "https://video.twimg.com/lowest-z.mp4"
        },
        {
            "bitrate": 3_000_000,
            "content_type": "video/mp4",
            "url": "https://video.twimg.com/lowest-a.mp4"
        }
    ]);

    let post = parse_syndication_payload(&payload.to_string()).unwrap();
    assert_eq!(
        post.attachments[0].url,
        "https://video.twimg.com/lowest-a.mp4"
    );
    assert_eq!(post.attachments[0].bitrate, Some(3_000_000));
}

#[test]
fn decodes_named_decimal_and_hex_entities_once() {
    let payload = PHOTO_FIXTURE.replace(
        "A &amp; B https://t.co/photo",
        "&lt;tag&gt; &quot;x&quot; &#39;y&#39; &#x1F680; &amp;amp;",
    );
    let payload = payload.replace("[0, 9]", "[0, 57]");

    let post = parse_syndication_payload(&payload).unwrap();
    assert_eq!(post.text, "<tag> \"x\" 'y' 🚀 &amp;");
}

#[test]
fn rejects_unavailable_and_malformed_payloads() {
    assert_eq!(
        parse_syndication_payload("{}"),
        Err(XSourceError::UnavailablePost)
    );
    assert_eq!(
        parse_syndication_payload(r#"{"__typename":"TweetUnavailable","reason":"Protected"}"#),
        Err(XSourceError::UnavailablePost)
    );
    assert_eq!(
        parse_syndication_payload("not json"),
        Err(XSourceError::InvalidJson)
    );

    let malformed = r#"{
        "__typename":"Tweet",
        "id_str":"123",
        "text":"hello",
        "created_at":"not-a-date",
        "user":{"name":"A","screen_name":"a","profile_image_url_https":"https://example.com/a.jpg"}
    }"#;
    assert!(matches!(
        parse_syndication_payload(malformed),
        Err(XSourceError::MalformedPayload("created_at"))
    ));
}

#[test]
fn rejects_a_video_without_a_progressive_mp4() {
    let payload = VIDEO_FIXTURE.replace("video/mp4", "application/x-mpegURL");
    assert_eq!(
        parse_syndication_payload(&payload),
        Err(XSourceError::MissingProgressiveVideo)
    );
}

#[test]
fn rejects_a_note_tweet_when_the_public_payload_only_contains_truncated_text() {
    assert_eq!(
        parse_syndication_payload(NOTE_TWEET_FIXTURE),
        Err(XSourceError::TruncatedPost)
    );
}

#[test]
fn rejects_an_x_article_wrapper_instead_of_saving_its_redirect_as_post_text() {
    assert_eq!(
        parse_syndication_payload(ARTICLE_FIXTURE),
        Err(XSourceError::UnsupportedArticle)
    );
}
