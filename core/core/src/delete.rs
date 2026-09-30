// SPDX-License-Identifier: MIT

//! Permanent deletion of a reading.
//!
//! Unlike archiving (which only flips the `archived` flag in the frontmatter),
//! deleting removes the reading's entire folder from disk — its `article.md`,
//! `assets/`, `highlights.md`, and `note.md` in one `remove_dir_all` — then
//! drops its row from the index. This is irreversible.

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::{
    locking::{lock_reading, ReadingLock},
    reconcile::apply_diffs,
    scanner::ScanDiff,
    LibraryRoot, ReadingKind,
};

/// Permanently delete the reading `id`: its whole folder and its index row.
/// Errors if the reading does not exist.
///
/// Deleting a folder is far blunter than deleting a single file, so this is
/// deliberately conservative about *what* it will remove. Because the library is
/// synced and externally writable, the on-disk tree is untrusted; the guardrails
/// ensure the target is exactly one reading's own folder:
///
/// - the id must look like a real reading id (non-empty, ASCII-alphanumeric), so
///   a crafted value can't smuggle `/` or `..` into the path and escape the
///   reading folder;
/// - the folder must contain an `article.md` whose frontmatter id **equals** the
///   requested id — so a divergent file (from an edit/sync that put a different
///   id in this folder) can never cause a delete of `id` to wipe another reading;
/// - the folder must sit *below* a fan-out bucket, never at `articles/` or a
///   bucket itself, so one delete can never wipe many readings at once; and
/// - the folder must not be a symlink, and its *canonical* path must resolve
///   beneath the canonical `articles/` — so a symlinked bucket or ancestor can't
///   redirect `remove_dir_all` outside the library.
pub fn delete_reading(library: &LibraryRoot, conn: &Connection, id: &str) -> Result<()> {
    if !is_valid_reading_id(id) {
        bail!("refusing to delete: invalid reading id {id:?}");
    }
    let lock = lock_reading(library, id)?;
    delete_reading_files_under_lock(library, id, &lock)?;
    apply_diffs(conn, &[ScanDiff::Removed(id.to_string())])
}

/// Delete an existing-library migration target only if it is still the same
/// lightweight, unenriched HTTP(S) link observed before the network request.
///
/// The check and removal share one reading lock. If browser capture, sync, or
/// another writer enriches/upgrades the reading while metadata is being
/// fetched, this returns `false` and leaves the newer file untouched. The note
/// snapshot distinguishes an absent sidecar from exact `note.md` bytes because
/// deleting the reading folder would otherwise discard a concurrent note edit.
pub fn delete_unenriched_link_files_if_unchanged(
    library: &LibraryRoot,
    id: &str,
    expected_url: &str,
    expected_article_sha256: &str,
    expected_note_sha256: Option<&str>,
) -> Result<bool> {
    if !is_valid_reading_id(id) {
        bail!("refusing to delete: invalid reading id {id:?}");
    }
    if expected_article_sha256.len() != 64
        || !expected_article_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("expected article snapshot must be 64 lowercase hexadecimal characters");
    }
    if expected_note_sha256.is_some_and(|expected| {
        expected.len() != 64
            || !expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        bail!("expected note snapshot must be 64 lowercase hexadecimal characters");
    }
    let expected_id = crate::url_id(expected_url)?;
    if expected_id != id {
        bail!("refusing to delete: expected URL does not match reading id");
    }
    let lock = lock_reading(library, id)?;
    let article_path = library.article_path(id);
    let article_bytes = match std::fs::read(&article_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if crate::sha256_hex(&article_bytes) != expected_article_sha256 {
        return Ok(false);
    }
    let note_matches = match std::fs::read(library.note_path(id)) {
        Ok(note_bytes) => {
            expected_note_sha256.is_some_and(|expected| crate::sha256_hex(&note_bytes) == expected)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            expected_note_sha256.is_none()
        }
        Err(error) => return Err(error.into()),
    };
    if !note_matches {
        return Ok(false);
    }
    let article = std::str::from_utf8(&article_bytes)?;
    let metadata = crate::parse_reading(article)?.metadata;
    let still_target = metadata.id == id
        && metadata.kind == ReadingKind::Article
        && metadata.lightweight
        && crate::url_id(&metadata.url).is_ok_and(|stored_id| stored_id == id)
        && metadata.preview_asset.is_none()
        && metadata.favicon_asset.is_none();
    if !still_target {
        return Ok(false);
    }
    delete_reading_files_under_lock(library, id, &lock)?;
    Ok(true)
}

fn delete_reading_files_under_lock(
    library: &LibraryRoot,
    id: &str,
    lock: &ReadingLock,
) -> Result<()> {
    lock.ensure_protects(library, id)?;

    let dir = library.reading_dir(id);
    let article = library.article_path(id);

    if !article.is_file() {
        bail!("reading not found: {id}");
    }

    // The file on disk must really be this reading: its frontmatter id has to
    // match the folder we are about to remove.
    match crate::read_metadata(&article) {
        Ok(meta) if meta.id == id => {}
        Ok(meta) => bail!(
            "refusing to delete {id}: frontmatter id {:?} in {} does not match",
            meta.id,
            article.display()
        ),
        Err(e) => bail!("refusing to delete {id}: cannot read frontmatter: {e}"),
    }

    // Structural (lexical) guard: `dir` must be nested two levels under articles/
    // (bucket → id), never the articles/ root or a bucket directory.
    let articles = library.articles_dir();
    let nested_two_levels = dir.starts_with(&articles)
        && dir != articles
        && dir.parent().is_some_and(|bucket| bucket != articles);
    if !nested_two_levels {
        bail!(
            "refusing to delete: {} is not a reading folder",
            dir.display()
        );
    }

    // Containment guard (resolves symlinks): the reading folder must not itself
    // be a symlink, its real path must sit beneath the real articles/ dir, and
    // real articles/ must in turn sit beneath the real library root. Anchoring
    // all the way up to the root means a symlinked bucket *or* a symlinked
    // articles/ itself can't send remove_dir_all outside the selected library.
    if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
        bail!("refusing to delete: {} is a symlink", dir.display());
    }
    let real_root = std::fs::canonicalize(library.path())?;
    let real_articles = std::fs::canonicalize(&articles)?;
    let real_dir = std::fs::canonicalize(&dir)?;
    let contained = real_articles.starts_with(&real_root)
        && real_articles != real_root
        && real_dir.starts_with(&real_articles)
        && real_dir != real_articles;
    if !contained {
        bail!(
            "refusing to delete: {} resolves outside the library",
            dir.display()
        );
    }

    std::fs::remove_dir_all(&real_dir)?;

    // If that was the last reading in its fan-out bucket, remove the now-empty
    // bucket directory too, so buckets don't linger as empty shells. `remove_dir`
    // only succeeds on an empty directory, so a bucket that still holds sibling
    // readings (or anything else) is left untouched — and a race that repopulates
    // it just makes this a no-op.
    if let Some(bucket) = real_dir.parent() {
        let _ = std::fs::remove_dir(bucket);
    }

    Ok(())
}

/// A reading id must be non-empty and ASCII-alphanumeric — true for both
/// content-addressed ids (64-char lowercase hex) and ULIDs (Crockford base32).
/// Rejecting anything else keeps a bad id from resolving the reading folder to a
/// path outside a single reading (no `/`, no `.`/`..`, no empty component).
fn is_valid_reading_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::{index::open, new_id, reconcile::rebuild, write_reading, LibraryRoot, Metadata};

    fn make_library(dir: &TempDir) -> LibraryRoot {
        fs::create_dir_all(dir.path().join("articles")).unwrap();
        LibraryRoot::new(dir.path()).unwrap()
    }

    fn meta(id: &str) -> Metadata {
        Metadata {
            format_version: 1,
            id: id.to_string(),
            kind: Default::default(),
            lightweight: false,
            url: "https://example.com".to_string(),
            media_url: None,
            preview_asset: None,
            favicon_asset: None,
            theme_color: None,
            canonical_url: "https://example.com".to_string(),
            title: "Test".to_string(),
            author: None,
            site: None,
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
            source_hash: String::new(),
        }
    }

    fn row_count(conn: &Connection, id: &str) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM readings WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn delete_removes_file_assets_and_row() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        // Simulate a stored asset for this reading.
        let assets = lib.assets_dir(&id);
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("image.png"), b"x").unwrap();
        // ...and a saved highlight, which lives outside the index.
        crate::highlights::add_highlight(&lib, &id, "a passage").unwrap();
        // Personal notes are sidecars too, and must leave with the reading.
        crate::set_note(&lib, &id, "A personal note").unwrap();
        rebuild(&conn, &lib).unwrap();
        assert_eq!(row_count(&conn, &id), 1);

        delete_reading(&lib, &conn, &id).unwrap();

        assert!(!lib.article_path(&id).exists());
        assert!(!assets.exists());
        assert!(!lib.highlights_path(&id).exists());
        assert!(!lib.note_path(&id).exists());
        assert_eq!(row_count(&conn, &id), 0);
    }

    #[test]
    fn conditional_link_delete_skips_any_changed_article_snapshot() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let url = "https://example.com/migration-target";
        let id = crate::url_id(url).unwrap();
        let mut metadata = meta(&id);
        metadata.lightweight = true;
        metadata.url = url.to_string();
        metadata.canonical_url = url.to_string();
        write_reading(
            &lib,
            metadata.clone(),
            "[Open link](<https://example.com>)".into(),
        )
        .unwrap();
        let planned_hash = crate::sha256_hex(&fs::read(lib.article_path(&id)).unwrap());

        // Keep every old structural predicate true while changing user-visible
        // content. Only the exact article hash protects this local edit.
        metadata.title = "Locally changed title".to_string();
        write_reading(&lib, metadata, "Locally changed body".into()).unwrap();

        assert!(
            !delete_unenriched_link_files_if_unchanged(&lib, &id, url, &planned_hash, None,)
                .unwrap()
        );
        assert!(lib.article_path(&id).is_file());
    }

    #[test]
    fn conditional_link_delete_requires_the_exact_note_snapshot() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let url = "https://example.com/note-protected-target";
        let id = crate::url_id(url).unwrap();
        let mut metadata = meta(&id);
        metadata.lightweight = true;
        metadata.url = url.to_string();
        metadata.canonical_url = url.to_string();
        metadata.author = Some("Previously captured author".to_string());
        metadata.theme_color = Some("#123456".to_string());
        metadata.excerpt = Some("Previously captured excerpt".to_string());
        metadata.lang = Some("en".to_string());
        write_reading(&lib, metadata, "[Open link](<https://example.com>)".into()).unwrap();
        let article_hash = crate::sha256_hex(&fs::read(lib.article_path(&id)).unwrap());

        crate::set_note(&lib, &id, "Added after planning").unwrap();
        assert!(
            !delete_unenriched_link_files_if_unchanged(&lib, &id, url, &article_hash, None,)
                .unwrap()
        );

        let added_hash = crate::sha256_hex(&fs::read(lib.note_path(&id)).unwrap());
        crate::set_note(&lib, &id, "Changed after planning").unwrap();
        assert!(!delete_unenriched_link_files_if_unchanged(
            &lib,
            &id,
            url,
            &article_hash,
            Some(&added_hash),
        )
        .unwrap());

        let changed_hash = crate::sha256_hex(&fs::read(lib.note_path(&id)).unwrap());
        crate::set_note(&lib, &id, "").unwrap();
        assert!(!delete_unenriched_link_files_if_unchanged(
            &lib,
            &id,
            url,
            &article_hash,
            Some(&changed_hash),
        )
        .unwrap());

        crate::set_note(&lib, &id, "Exact planned note").unwrap();
        let exact_hash = crate::sha256_hex(&fs::read(lib.note_path(&id)).unwrap());
        assert!(delete_unenriched_link_files_if_unchanged(
            &lib,
            &id,
            url,
            &article_hash,
            Some(&exact_hash),
        )
        .unwrap());
        assert!(!lib.reading_dir(&id).exists());
    }

    #[test]
    fn conditional_link_delete_removes_the_unchanged_target() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let url = "https://example.com/dead-target";
        let id = crate::url_id(url).unwrap();
        let mut metadata = meta(&id);
        metadata.lightweight = true;
        metadata.url = url.to_string();
        metadata.canonical_url = url.to_string();
        write_reading(&lib, metadata, "[Open link](<https://example.com>)".into()).unwrap();
        let planned_hash = crate::sha256_hex(&fs::read(lib.article_path(&id)).unwrap());

        assert!(
            delete_unenriched_link_files_if_unchanged(&lib, &id, url, &planned_hash, None,)
                .unwrap()
        );
        assert!(!lib.article_path(&id).exists());
    }

    #[test]
    fn delete_succeeds_without_assets() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        delete_reading(&lib, &conn, &id).unwrap();
        assert!(!lib.article_path(&id).exists());
        assert_eq!(row_count(&conn, &id), 0);
    }

    #[test]
    fn delete_removes_the_whole_reading_folder() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        crate::highlights::add_highlight(&lib, &id, "a passage").unwrap();
        rebuild(&conn, &lib).unwrap();
        assert!(lib.reading_dir(&id).is_dir());

        delete_reading(&lib, &conn, &id).unwrap();

        // The entire folder is gone — article, assets, and highlights with it.
        assert!(!lib.reading_dir(&id).exists());
        assert_eq!(row_count(&conn, &id), 0);
    }

    #[test]
    fn delete_leaves_a_bucket_sibling_untouched() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        // Two distinct readings whose ids share the same first two characters, so
        // they land in the *same* fan-out bucket (articles/ab/) as sibling folders.
        let a = "ab00000000000000000000000000000000000000000000000000000000000001";
        let b = "ab00000000000000000000000000000000000000000000000000000000000002";
        write_reading(&lib, meta(a), "reading a".into()).unwrap();
        write_reading(&lib, meta(b), "reading b".into()).unwrap();
        rebuild(&conn, &lib).unwrap();
        let bucket = lib.reading_dir(a).parent().unwrap().to_path_buf();
        assert_eq!(
            bucket,
            lib.reading_dir(b).parent().unwrap(),
            "the two readings must share one bucket directory"
        );
        assert_eq!(row_count(&conn, a), 1);
        assert_eq!(row_count(&conn, b), 1);

        delete_reading(&lib, &conn, a).unwrap();

        // A's folder and row are gone; its bucket sibling B is fully intact —
        // deleting one reading in a bucket never touches the other, and the
        // shared bucket directory is kept because it still holds B.
        assert!(!lib.reading_dir(a).exists());
        assert_eq!(row_count(&conn, a), 0);
        assert!(bucket.is_dir(), "the shared bucket must survive");
        assert!(lib.article_path(b).is_file(), "B's article must survive");
        assert!(lib.reading_dir(b).is_dir(), "B's folder must survive");
        assert_eq!(row_count(&conn, b), 1);
    }

    #[test]
    fn deleting_the_last_reading_removes_the_empty_bucket() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        // The only reading in its bucket.
        let id = "cd00000000000000000000000000000000000000000000000000000000000001";
        write_reading(&lib, meta(id), "only reading".into()).unwrap();
        rebuild(&conn, &lib).unwrap();
        let bucket = lib.reading_dir(id).parent().unwrap().to_path_buf();
        assert!(bucket.is_dir());

        delete_reading(&lib, &conn, id).unwrap();

        // The reading and its now-empty bucket are both gone, but articles/ stays.
        assert!(!lib.reading_dir(id).exists());
        assert!(!bucket.exists(), "the emptied bucket is cleaned up");
        assert!(lib.articles_dir().is_dir(), "articles/ itself is kept");
        assert_eq!(row_count(&conn, id), 0);
    }

    #[test]
    fn delete_unknown_id_errors() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();
        // A well-formed id that simply isn't present hits the not-found guard.
        assert!(delete_reading(&lib, &conn, &new_id()).is_err());
    }

    #[test]
    fn delete_refuses_when_frontmatter_id_diverges_from_folder() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        // Put an article whose frontmatter says B inside A's folder — the kind of
        // divergence an external edit or sync could create.
        let a = new_id();
        let b = new_id();
        let folder_a = lib.reading_dir(&a);
        fs::create_dir_all(&folder_a).unwrap();
        let content = crate::render_reading(&crate::Reading {
            metadata: meta(&b),
            body: "body".into(),
        })
        .unwrap();
        fs::write(folder_a.join("article.md"), content).unwrap();

        // Deleting A must refuse: the file in A's folder is really B.
        assert!(delete_reading(&lib, &conn, &a).is_err());
        assert!(lib.article_path(&a).is_file(), "A's folder must survive");
    }

    #[cfg(unix)]
    #[test]
    fn delete_refuses_symlinked_bucket_escaping_the_library() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        // A real, well-formed reading folder that lives OUTSIDE the library.
        let outside = TempDir::new().unwrap();
        let id = new_id();
        let outside_reading = outside.path().join(&id);
        fs::create_dir_all(&outside_reading).unwrap();
        let content = crate::render_reading(&crate::Reading {
            metadata: meta(&id),
            body: "body".into(),
        })
        .unwrap();
        fs::write(outside_reading.join("article.md"), content).unwrap();

        // Point the in-library fan-out bucket at that outside directory, so the
        // reading resolves through the symlink (frontmatter id matches, too).
        let bucket = lib.articles_dir().join(&id[..2]);
        symlink(outside.path(), &bucket).unwrap();
        assert!(lib.article_path(&id).is_file(), "resolves via the symlink");

        // Delete must refuse: the folder's real path is outside the library.
        assert!(delete_reading(&lib, &conn, &id).is_err());
        assert!(
            outside_reading.join("article.md").is_file(),
            "content outside the library must be left untouched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_refuses_when_articles_is_symlinked_outside_the_library() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        // A library root whose `articles/` does not exist yet — we replace it
        // with a symlink, so the whole articles tree resolves outside the root.
        let lib_root = dir.path().join("library");
        fs::create_dir_all(&lib_root).unwrap();
        let lib = LibraryRoot::new(&lib_root).unwrap();
        let conn = open(&dir.path().join("index.db")).unwrap();

        // A well-formed reading in a fake articles tree OUTSIDE the library.
        let outside = TempDir::new().unwrap();
        let id = new_id();
        let outside_reading = outside.path().join(&id[..2]).join(&id);
        fs::create_dir_all(&outside_reading).unwrap();
        let content = crate::render_reading(&crate::Reading {
            metadata: meta(&id),
            body: "body".into(),
        })
        .unwrap();
        fs::write(outside_reading.join("article.md"), content).unwrap();

        // Point <library>/articles at that outside tree.
        symlink(outside.path(), lib.articles_dir()).unwrap();
        assert!(
            lib.article_path(&id).is_file(),
            "resolves via the articles symlink"
        );

        // Delete must refuse: articles/ resolves outside the selected library.
        assert!(delete_reading(&lib, &conn, &id).is_err());
        assert!(
            outside_reading.join("article.md").is_file(),
            "content outside the library must be left untouched"
        );
    }

    #[test]
    fn delete_rejects_malformed_ids_without_touching_disk() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        // Seed one real reading so there is something a traversal could target.
        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // Ids that could escape a single reading folder are refused outright.
        for bad in ["", "..", "../../..", "a/b", "8f/8f.md", "."] {
            assert!(
                delete_reading(&lib, &conn, bad).is_err(),
                "malformed id {bad:?} must be refused"
            );
        }

        // The real reading is untouched.
        assert!(lib.article_path(&id).is_file());
        assert_eq!(row_count(&conn, &id), 1);
    }
}
