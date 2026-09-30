// SPDX-License-Identifier: MIT

use std::fs;
use std::io::Write as _;

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::frontmatter::{read_metadata, render_reading};
use crate::locking::{lock_reading, ReadingLock};
use crate::types::{LibraryRoot, Metadata, Reading, ReadingKind};

/// Write a reading to the library atomically (temp-file + rename).
///
/// Computes and sets `source_hash` from the body before writing.
/// Creates the reading's self-contained folder `articles/<prefix>/<id>/` and its
/// `assets/` sub-directory.
pub fn write_reading(library: &LibraryRoot, metadata: Metadata, body: String) -> Result<Reading> {
    let lock = lock_reading(library, &metadata.id)?;
    write_reading_under_lock(library, metadata, body, &lock)
}

/// Atomic writer for callers that already hold this reading's advisory lock.
/// Keeping this separate avoids trying to acquire the same non-reentrant lock
/// twice inside read-modify-write operations.
pub(crate) fn write_reading_under_lock(
    library: &LibraryRoot,
    mut metadata: Metadata,
    body: String,
    lock: &ReadingLock,
) -> Result<Reading> {
    lock.ensure_protects(library, &metadata.id)?;
    metadata.source_hash = format!("sha256:{}", sha256_hex(body.as_bytes()));

    let reading = Reading { metadata, body };
    let content = render_reading(&reading)?;

    let article_path = library.article_path(&reading.metadata.id);
    // Create the reading's folder (articles/<prefix>/<id>/) — the article file's
    // parent — plus its assets dir, so both exist before the write.
    fs::create_dir_all(library.assets_dir(&reading.metadata.id))?;

    let tmp_path = article_path.with_file_name("article.md.tmp");

    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, &article_path)?;

    Ok(reading)
}

/// Content-addressed lookup: is a page with this URL already saved? Returns its id.
///
/// The id *is* `SHA256(normalize(url))`, so this hashes the URL to the id and
/// stats the single file it would live at — no directory scan. This is what both
/// the save-time duplicate check and the toolbar's "already saved?" check use.
///
/// Identity is the normalized *visited* URL, not the page's `<link rel=canonical>`
/// — the toolbar only knows the tab URL and can't run extraction to discover a
/// canonical link. As a cheap safety net against the (astronomically unlikely)
/// hash collision, the stored `url` is read from the frontmatter and confirmed to
/// normalize to the same key before reporting a match.
pub fn find_by_url(library: &LibraryRoot, url: &str) -> Result<Option<String>> {
    let id = match crate::url_id(url) {
        Ok(id) => id,
        Err(_) => return Ok(None), // unparseable URL → can't be content-addressed
    };
    let path = library.article_path(&id);
    if !path.is_file() {
        return Ok(None);
    }
    // Collision guard: confirm the file really is this URL. Reading only the
    // frontmatter keeps this cheap. An unreadable header falls back to trusting
    // the hash rather than hiding a genuine match behind a parse error.
    match read_metadata(&path) {
        Ok(m) if norm_key(&m.url) == norm_key(url) => Ok(Some(id)),
        Ok(_) => Ok(None),
        Err(_) => Ok(Some(id)),
    }
}

/// Content-addressed lookup for an image or video saved from a source page.
///
/// As with [`find_by_url`], this is one hash plus one stat. The frontmatter
/// check guards against a hash collision and confirms all identity components.
pub fn find_by_media(
    library: &LibraryRoot,
    kind: ReadingKind,
    source_page_url: &str,
    media_url: &str,
) -> Result<Option<String>> {
    let id = match crate::media_id(kind, source_page_url, media_url) {
        Ok(id) => id,
        Err(_) => return Ok(None),
    };
    let path = library.article_path(&id);
    if !path.is_file() {
        return Ok(None);
    }

    match read_metadata(&path) {
        Ok(metadata)
            if metadata.kind == kind
                && metadata.media_url.as_deref().is_some_and(|stored| {
                    crate::media_id(metadata.kind, &metadata.url, stored)
                        .is_ok_and(|stored_id| stored_id == id)
                }) =>
        {
            Ok(Some(id))
        }
        Ok(_) => Ok(None),
        Err(_) => Ok(Some(id)),
    }
}

/// Normalize a URL for matching, falling back to the raw string when it can't be
/// parsed (e.g. a non-http scheme) so exact-equality matching still works.
fn norm_key(url: &str) -> String {
    crate::normalize_url(url).unwrap_or_else(|_| url.to_string())
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Metadata;
    use tempfile::TempDir;

    fn tmp_library() -> (TempDir, LibraryRoot) {
        let dir = TempDir::new().unwrap();
        let lib = LibraryRoot::new(dir.path()).unwrap();
        (dir, lib)
    }

    /// Metadata for a page saved from `url`, with the content-addressed id the
    /// save path would assign (id == url_id(url)) so on-disk lookups resolve.
    fn metadata_for(url: &str) -> Metadata {
        Metadata {
            format_version: 1,
            id: crate::url_id(url).unwrap(),
            kind: Default::default(),
            lightweight: false,
            url: url.to_string(),
            media_url: None,
            preview_asset: None,
            favicon_asset: None,
            theme_color: None,
            canonical_url: url.to_string(),
            title: "Test".to_string(),
            author: None,
            site: Some("example.com".to_string()),
            source_profile: None,
            saved_at: "2026-06-13T15:00:00Z".to_string(),
            read_at: None,
            archived: false,
            favorite: false,
            rating: 0,
            tags: vec![],
            machine_tags: vec![],
            excluded_machine_tags: vec![],
            excerpt: None,
            word_count: None,
            lang: None,
            source_hash: String::new(), // set by write_reading
        }
    }

    fn sample_metadata() -> Metadata {
        metadata_for("https://example.com/post")
    }

    #[test]
    fn writes_article_file() {
        let (_dir, lib) = tmp_library();
        let meta = sample_metadata();
        let id = meta.id.clone();

        write_reading(&lib, meta, "# Test\n\nContent.\n".to_string()).unwrap();

        assert!(lib.article_path(&id).exists());
        assert!(lib.assets_dir(&id).is_dir());
    }

    #[test]
    fn same_prefix_readings_share_one_bucket() {
        let (_dir, lib) = tmp_library();

        // Two readings whose ids share the same first two characters, so they
        // fan out into the same bucket directory.
        let mut a = metadata_for("https://a.example.com");
        a.id = "ab00000000000000000000000000000000000000000000000000000000000001".to_string();
        let mut b = metadata_for("https://b.example.com");
        b.id = "ab00000000000000000000000000000000000000000000000000000000000002".to_string();

        write_reading(&lib, a.clone(), "reading a".into()).unwrap();
        let bucket = lib.reading_dir(&a.id).parent().unwrap().to_path_buf();
        assert!(bucket.is_dir(), "the first save creates the fan-out bucket");
        assert!(lib.article_path(&a.id).is_file());

        write_reading(&lib, b.clone(), "reading b".into()).unwrap();
        assert_eq!(
            lib.reading_dir(&b.id).parent().unwrap(),
            bucket,
            "the second reading joins the existing bucket"
        );
        // The second save leaves the first reading in place.
        assert!(
            lib.article_path(&a.id).is_file(),
            "the first reading survives the second save"
        );
        assert!(lib.article_path(&b.id).is_file());
    }

    #[test]
    fn source_hash_is_set() {
        let (_dir, lib) = tmp_library();
        let meta = sample_metadata();
        let body = "# Test\n\nContent.\n".to_string();

        let reading = write_reading(&lib, meta, body.clone()).unwrap();

        let expected = format!("sha256:{}", sha256_hex(body.as_bytes()));
        assert_eq!(reading.metadata.source_hash, expected);
    }

    #[test]
    fn find_by_url_detects_saved_page() {
        let (_dir, lib) = tmp_library();
        let url = "https://example.com/post";
        let meta = metadata_for(url);
        let id = meta.id.clone();
        write_reading(&lib, meta, "# Test\n".to_string()).unwrap();

        let found = find_by_url(&lib, url).unwrap();
        assert_eq!(found.as_deref(), Some(id.as_str()));
        // The id really is the content address of the URL.
        assert_eq!(id, crate::url_id(url).unwrap());
    }

    #[test]
    fn find_by_url_returns_none_for_new_url() {
        let (_dir, lib) = tmp_library();
        assert!(find_by_url(&lib, "https://example.com/new")
            .unwrap()
            .is_none());
    }

    #[test]
    fn find_by_url_ignores_tracking_params() {
        // Visiting a saved page with a utm tag must still resolve to it.
        let (_dir, lib) = tmp_library();
        let url = "https://paulgraham.com/taste.html";
        write_reading(&lib, metadata_for(url), "# Taste\n".to_string()).unwrap();

        assert!(
            find_by_url(&lib, "https://paulgraham.com/taste.html?utm_source=oia")
                .unwrap()
                .is_some(),
            "utm-tagged visit still matches the saved page"
        );
        assert!(
            find_by_url(&lib, "https://paulgraham.com/articles.html?id=2")
                .unwrap()
                .is_none(),
            "a genuinely different page (meaningful query) must not match"
        );
    }

    #[test]
    fn find_by_url_keys_on_visited_url_not_canonical() {
        // Identity is the normalized *visited* URL. A page saved from one address
        // that declares a different rel=canonical is found by the visited URL it
        // was saved from — not by its canonical. This is the accepted trade-off
        // of content-addressing (worst case: the same content saved twice).
        let (_dir, lib) = tmp_library();
        let mut meta = metadata_for("https://example.com/from-feed");
        meta.canonical_url = "https://example.com/post".to_string();
        write_reading(&lib, meta, "# Test\n".to_string()).unwrap();

        assert!(
            find_by_url(&lib, "https://example.com/from-feed")
                .unwrap()
                .is_some(),
            "found by the visited url it was saved from"
        );
        assert!(
            find_by_url(&lib, "https://example.com/post")
                .unwrap()
                .is_none(),
            "the declared canonical is not the key"
        );
    }

    #[test]
    fn find_by_media_distinguishes_items_on_one_page() {
        let (_dir, lib) = tmp_library();
        let source = "https://example.com/gallery";
        let first_url = "https://cdn.example.com/first.jpg";
        let second_url = "https://cdn.example.com/second.jpg";
        let mut metadata = metadata_for(source);
        metadata.kind = ReadingKind::Image;
        metadata.media_url = Some(first_url.to_string());
        metadata.id = crate::media_id(metadata.kind, source, first_url).unwrap();
        write_reading(&lib, metadata, "![first](assets/first.jpg)".into()).unwrap();

        assert!(find_by_media(&lib, ReadingKind::Image, source, first_url)
            .unwrap()
            .is_some());
        assert!(find_by_media(&lib, ReadingKind::Image, source, second_url)
            .unwrap()
            .is_none());
    }

    #[test]
    fn find_by_media_matches_local_asset_hash_across_extensions() {
        let (_dir, lib) = tmp_library();
        let source = "https://example.com/watch";
        let hash = "ab".repeat(32);
        let stored = format!("cuttings-asset:assets/{hash}.mov");
        let equivalent = format!("cuttings-asset:assets/{hash}.mp4");
        let mut metadata = metadata_for(source);
        metadata.kind = ReadingKind::Video;
        metadata.media_url = Some(stored.clone());
        metadata.id = crate::media_id(metadata.kind, source, &stored).unwrap();
        let id = metadata.id.clone();
        write_reading(&lib, metadata, "[Play](assets/video.mov)".into()).unwrap();

        assert_eq!(
            find_by_media(&lib, ReadingKind::Video, source, &equivalent).unwrap(),
            Some(id)
        );
    }
}
