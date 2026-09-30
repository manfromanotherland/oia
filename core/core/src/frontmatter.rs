// SPDX-License-Identifier: MIT

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{anyhow, Result};

use crate::types::{Metadata, Reading};

/// Parse a reading from a file's raw text content.
///
/// Expected format:
/// ```text
/// ---
/// <YAML frontmatter>
/// ---
///
/// <Markdown body>
/// ```
pub fn parse_reading(content: &str) -> Result<Reading> {
    let rest = content
        .strip_prefix("---\n")
        .ok_or_else(|| anyhow!("missing opening frontmatter fence"))?;

    let close = rest
        .find("\n---\n")
        .ok_or_else(|| anyhow!("missing closing frontmatter fence"))?;

    let yaml_str = &rest[..close];
    let after_fence = &rest[close + 5..]; // skip "\n---\n"
    let body = after_fence.trim_start_matches('\n').to_string();

    // Parse once into a generic value so we can read any legacy `read: true`
    // flag from files written before `read_at` existed, then into Metadata.
    let value: serde_yaml::Value =
        serde_yaml::from_str(yaml_str).map_err(|e| anyhow!("invalid frontmatter: {e}"))?;
    let legacy_read = value.get("read").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut metadata: Metadata =
        serde_yaml::from_value(value).map_err(|e| anyhow!("invalid frontmatter: {e}"))?;

    // Forward-migrate the dropped `read` boolean: a legacy read article with no
    // `read_at` is treated as read at its save time. The next write drops the
    // `read` key entirely (it is no longer part of Metadata).
    if metadata.read_at.is_none() && legacy_read {
        metadata.read_at = Some(metadata.saved_at.clone());
    }

    Ok(Reading { metadata, body })
}

/// Read only a reading file's frontmatter, without loading the body.
///
/// Reads line by line and stops at the closing `---` fence, so a
/// multi-thousand-word article costs only its ~20 header lines instead of the
/// whole file. Used by scans that only need metadata (dedup and the "already
/// saved?" toolbar check), where slurping every body would read hundreds of MB
/// across a large library just to compare a couple of URL fields.
///
/// Parsing is delegated to [`parse_reading`] on the captured header, so the
/// frontmatter semantics (including the legacy `read` migration) stay identical
/// to a full read.
pub fn read_metadata(path: &Path) -> Result<Metadata> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut header = String::new();
    let mut line = String::new();
    let mut fences = 0;

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            // EOF before the second fence — hand the partial header to
            // parse_reading, which reports the missing-fence error.
            break;
        }
        header.push_str(&line);
        if line.trim_end() == "---" {
            fences += 1;
            if fences == 2 {
                break;
            }
        }
    }

    parse_reading(&header).map(|r| r.metadata)
}

/// Render a reading back to its on-disk text format.
pub fn render_reading(reading: &Reading) -> Result<String> {
    let yaml = serde_yaml::to_string(&reading.metadata)
        .map_err(|e| anyhow!("frontmatter serialization failed: {e}"))?;

    let body = reading.body.trim_end_matches('\n').to_string() + "\n";
    Ok(format!("---\n{}---\n\n{body}", yaml))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Metadata, SourceAttachment, SourceProfile};
    use proptest::prelude::*;

    fn sample_metadata(read: bool, archived: bool, favorite: bool) -> Metadata {
        Metadata {
            format_version: 1,
            id: "01J9Z8X7Q2VBKN3P4HXYZ01AB".to_string(),
            kind: Default::default(),
            lightweight: false,
            url: "https://example.com/article".to_string(),
            media_url: None,
            preview_asset: None,
            favicon_asset: None,
            theme_color: None,
            canonical_url: "https://example.com/article".to_string(),
            title: "Test Article".to_string(),
            author: Some("Jane Doe".to_string()),
            site: Some("example.com".to_string()),
            source_profile: None,
            saved_at: "2026-06-13T15:00:00Z".to_string(),
            read_at: read.then(|| "2026-06-13T16:00:00.000Z".to_string()),
            archived,
            favorite,
            rating: 0,
            tags: vec!["rust".to_string(), "local-first".to_string()],
            machine_tags: vec![],
            excluded_machine_tags: vec![],
            excerpt: Some("A short excerpt.".to_string()),
            word_count: Some(1234),
            lang: Some("en".to_string()),
            source_hash: "sha256:abc123".to_string(),
        }
    }

    #[test]
    fn round_trip_full() {
        let meta = sample_metadata(false, false, true);
        let reading = Reading {
            metadata: meta.clone(),
            body: "# Test Article\n\nBody paragraph.\n".to_string(),
        };
        let rendered = render_reading(&reading).unwrap();
        let parsed = parse_reading(&rendered).unwrap();
        assert_eq!(parsed.metadata, meta);
        assert_eq!(parsed.body.trim(), reading.body.trim());
    }

    #[test]
    fn round_trip_media_fields() {
        let mut meta = sample_metadata(false, false, false);
        meta.kind = crate::ReadingKind::Video;
        meta.media_url = Some("https://cdn.example.com/clip.mp4".into());
        meta.preview_asset = Some("assets/poster.jpg".into());
        meta.favicon_asset = Some("assets/favicon.ico".into());
        let reading = Reading {
            metadata: meta.clone(),
            body: "![Poster](assets/poster.jpg)\n".into(),
        };

        let rendered = render_reading(&reading).unwrap();
        assert!(rendered.contains("kind: video"));
        assert!(rendered.contains("media_url: https://cdn.example.com/clip.mp4"));
        assert!(rendered.contains("preview_asset: assets/poster.jpg"));
        assert!(rendered.contains("favicon_asset: assets/favicon.ico"));
        assert_eq!(parse_reading(&rendered).unwrap().metadata, meta);
    }

    #[test]
    fn round_trip_source_profile_preserves_open_strings_and_attachment_order() {
        let mut meta = sample_metadata(false, false, false);
        meta.source_profile = Some(SourceProfile {
            version: 1,
            source_type: "future-social-kind".into(),
            provider: "future-provider".into(),
            source_id: "2102505743278829840".into(),
            author_handle: "@benspringwater".into(),
            published_at: Some("2026-09-22T18:42:00.000Z".into()),
            avatar_asset: Some("assets/avatar.jpg".into()),
            attachments: vec![
                SourceAttachment {
                    kind: "future-image-kind".into(),
                    asset: "assets/first.jpg".into(),
                    poster_asset: None,
                    content_type: Some("image/jpeg".into()),
                    width: Some(1200),
                    height: Some(800),
                    alt: Some("First image".into()),
                },
                SourceAttachment {
                    kind: "future-video-kind".into(),
                    asset: "assets/second.mp4".into(),
                    poster_asset: Some("assets/second-poster.jpg".into()),
                    content_type: Some("video/mp4".into()),
                    width: Some(1920),
                    height: Some(1080),
                    alt: None,
                },
            ],
        });
        let reading = Reading {
            metadata: meta.clone(),
            body: "Post body.\n".into(),
        };

        let rendered = render_reading(&reading).unwrap();
        let parsed = parse_reading(&rendered).unwrap();

        assert_eq!(parsed.metadata, meta);
        assert!(rendered.contains("source_type: future-social-kind"));
        assert!(rendered.contains("provider: future-provider"));
        let attachments = &parsed.metadata.source_profile.unwrap().attachments;
        assert_eq!(attachments[0].asset, "assets/first.jpg");
        assert_eq!(attachments[1].asset, "assets/second.mp4");
    }

    #[test]
    fn round_trip_optional_fields_absent() {
        let mut meta = sample_metadata(true, false, false);
        meta.author = None;
        meta.site = None;
        meta.excerpt = None;
        meta.word_count = None;
        meta.lang = None;

        let reading = Reading {
            metadata: meta.clone(),
            body: "# Minimal\n".to_string(),
        };
        let rendered = render_reading(&reading).unwrap();
        let parsed = parse_reading(&rendered).unwrap();
        assert_eq!(parsed.metadata, meta);
    }

    #[test]
    fn legacy_read_true_migrates_to_read_at() {
        // A file written before `read_at` existed: it carries `read: true` and
        // no `read_at`. Parsing must treat it as read, stamped at save time.
        let content = "\
---
format_version: 1
id: 01J9Z8X7Q2VBKN3P4HXYZ01AB
url: https://example.com/article
canonical_url: https://example.com/article
title: Old Article
saved_at: 2026-06-13T15:00:00Z
read: true
archived: false
favorite: false
tags: []
source_hash: sha256:abc
---

Body.
";
        let parsed = parse_reading(content).unwrap();
        assert_eq!(
            parsed.metadata.read_at.as_deref(),
            Some("2026-06-13T15:00:00Z")
        );
        assert_eq!(parsed.metadata.kind, crate::ReadingKind::Article);
        assert_eq!(parsed.metadata.media_url, None);
        assert_eq!(parsed.metadata.preview_asset, None);
        assert_eq!(parsed.metadata.favicon_asset, None);
    }

    #[test]
    fn legacy_read_false_stays_unread() {
        let content = "\
---
format_version: 1
id: 01J9Z8X7Q2VBKN3P4HXYZ01AB
url: https://example.com/article
canonical_url: https://example.com/article
title: Old Article
saved_at: 2026-06-13T15:00:00Z
read: false
archived: false
favorite: false
tags: []
source_hash: sha256:abc
---

Body.
";
        let parsed = parse_reading(content).unwrap();
        assert_eq!(parsed.metadata.read_at, None);
    }

    #[test]
    fn read_metadata_matches_full_parse() {
        // read_metadata must return exactly what a full parse_reading would,
        // having read only the header.
        let meta = sample_metadata(true, false, true);
        let reading = Reading {
            metadata: meta.clone(),
            body: "# Long Article\n\n".to_string() + &"word ".repeat(5000),
        };
        let rendered = render_reading(&reading).unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("article.md");
        std::fs::write(&path, &rendered).unwrap();

        assert_eq!(read_metadata(&path).unwrap(), meta);
    }

    #[test]
    fn read_metadata_ignores_body_fences() {
        // A `---` line inside the body (e.g. a Markdown thematic break) must not
        // confuse the header scan: it stops at the *frontmatter's* closing fence.
        let meta = sample_metadata(false, false, false);
        let reading = Reading {
            metadata: meta.clone(),
            body: "Intro.\n\n---\n\nA section after a horizontal rule.\n".to_string(),
        };
        let rendered = render_reading(&reading).unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("article.md");
        std::fs::write(&path, &rendered).unwrap();

        assert_eq!(read_metadata(&path).unwrap(), meta);
    }

    #[test]
    fn read_metadata_errors_on_unfenced_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("not-a-reading.md");
        std::fs::write(&path, "just some text, no frontmatter\n").unwrap();

        assert!(read_metadata(&path).is_err());
    }

    proptest! {
        #[test]
        fn bool_fields_round_trip(
            read in proptest::bool::ANY,
            archived in proptest::bool::ANY,
            favorite in proptest::bool::ANY,
            word_count in proptest::option::of(0u32..100_000u32),
        ) {
            let mut meta = sample_metadata(read, archived, favorite);
            meta.word_count = word_count;

            let reading = Reading {
                metadata: meta.clone(),
                body: "# Test\n\nBody.\n".to_string(),
            };
            let rendered = render_reading(&reading).unwrap();
            let parsed = parse_reading(&rendered).unwrap();

            prop_assert_eq!(parsed.metadata.read_at, meta.read_at);
            prop_assert_eq!(parsed.metadata.archived, meta.archived);
            prop_assert_eq!(parsed.metadata.favorite, meta.favorite);
            prop_assert_eq!(parsed.metadata.word_count, meta.word_count);
        }
    }
}
