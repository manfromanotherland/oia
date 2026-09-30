// SPDX-License-Identifier: MIT

//! Mutations for the boolean status flags: read, archived, favorite.
//!
//! Every setter follows the same pattern: update the `.md` frontmatter
//! (source of truth), then sync the index row via `apply_diffs`.

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::{
    locking::lock_reading,
    parse_reading,
    reconcile::apply_diffs,
    scanner::{ScanDiff, ScannedReading},
    writer::write_reading_under_lock,
    LibraryRoot,
};

/// Mark a reading as read (`true`) or unread (`false`).
///
/// Read state is carried entirely by `read_at`: marking read stamps it with the
/// current UTC time (overwriting any earlier value, so it always reflects the
/// most recent time marked read); marking unread clears it back to `None`.
pub fn set_read(library: &LibraryRoot, conn: &Connection, id: &str, read: bool) -> Result<()> {
    update_flag(library, conn, id, |m| {
        m.read_at = if read {
            Some(crate::time::now_utc_iso())
        } else {
            None
        };
    })
}

/// Archive (`true`) or un-archive (`false`) a reading.
pub fn set_archived(
    library: &LibraryRoot,
    conn: &Connection,
    id: &str,
    archived: bool,
) -> Result<()> {
    update_flag(library, conn, id, |m| m.archived = archived)
}

/// Mark a reading as a favorite (`true`) or remove that mark (`false`).
pub fn set_favorite(
    library: &LibraryRoot,
    conn: &Connection,
    id: &str,
    favorite: bool,
) -> Result<()> {
    update_flag(library, conn, id, |m| m.favorite = favorite)
}

/// Apply an arbitrary metadata mutation to a reading: update the `.md`
/// frontmatter (source of truth), then sync the index row. Shared by the
/// status setters and `set_rating`.
pub(crate) fn update_flag<F>(
    library: &LibraryRoot,
    conn: &Connection,
    id: &str,
    apply: F,
) -> Result<()>
where
    F: FnOnce(&mut crate::Metadata),
{
    let lock = lock_reading(library, id)?;
    let path = library.article_path(id);
    if !path.is_file() {
        bail!("reading not found: {id}");
    }

    let content = std::fs::read_to_string(&path)?;
    let mut reading = parse_reading(&content)?;

    apply(&mut reading.metadata);

    let written = write_reading_under_lock(library, reading.metadata, reading.body, &lock)?;

    // Re-read from disk so the ScannedReading reflects the actual file state.
    let updated_path = library.article_path(&written.metadata.id);
    let updated_content = std::fs::read_to_string(&updated_path)?;
    let updated = parse_reading(&updated_content)?;
    let modified_at = std::fs::metadata(&updated_path)?.modified()?;
    let visual_asset = updated.metadata.preview_asset.as_deref().and_then(|path| {
        if matches!(
            updated.metadata.kind,
            crate::ReadingKind::Article | crate::ReadingKind::Image
        ) {
            crate::visual_index::inspect_image_asset(library, &updated.metadata.id, path).ok()
        } else {
            crate::visual_index::inspect_asset(library, &updated.metadata.id, path).ok()
        }
    });
    let media_aspect_ratio = crate::scanner::inspect_media_aspect_ratio(
        library,
        &updated.metadata,
        visual_asset.as_ref(),
    );

    let scanned = ScannedReading {
        id: updated.metadata.id.clone(),
        source_hash: updated.metadata.source_hash.clone(),
        modified_at,
        path: updated_path,
        has_note: crate::scanner::note_file_exists(library, &updated.metadata.id),
        visual_asset,
        media_aspect_ratio,
        body: updated.body,
        metadata: updated.metadata,
    };

    apply_diffs(conn, &[ScanDiff::Changed(scanned)])
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

    fn setup() -> (TempDir, Connection) {
        let dir = TempDir::new().unwrap();
        let conn = open(&dir.path().join("index.db")).unwrap();
        (dir, conn)
    }

    fn read_flag(conn: &Connection, id: &str, col: &str) -> bool {
        let sql = format!("SELECT {col} FROM readings WHERE id = ?1");
        conn.query_row(&sql, rusqlite::params![id], |r| r.get::<_, i32>(0))
            .unwrap()
            != 0
    }

    fn read_at_is_set(conn: &Connection, id: &str) -> bool {
        conn.query_row(
            "SELECT read_at IS NOT NULL FROM readings WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get::<_, i32>(0),
        )
        .unwrap()
            != 0
    }

    // ── read / unread ────────────────────────────────────────────────────────

    #[test]
    fn set_read_updates_frontmatter() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        set_read(&lib, &conn, &id, true).unwrap();

        let content = fs::read_to_string(lib.article_path(&id)).unwrap();
        // Read state is the presence of `read_at`, not a `read:` boolean.
        assert!(content.contains("read_at:"));
        assert!(!content.contains("\nread:"));
    }

    #[test]
    fn set_read_updates_index() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        set_read(&lib, &conn, &id, true).unwrap();
        assert!(read_at_is_set(&conn, &id));

        set_read(&lib, &conn, &id, false).unwrap();
        assert!(!read_at_is_set(&conn, &id));
    }

    // ── archived ─────────────────────────────────────────────────────────────

    #[test]
    fn set_archived_updates_frontmatter_and_index() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        set_archived(&lib, &conn, &id, true).unwrap();

        let content = fs::read_to_string(lib.article_path(&id)).unwrap();
        assert!(content.contains("archived: true"));
        assert!(read_flag(&conn, &id, "archived"));
    }

    #[test]
    fn set_archived_false_restores_reading() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        let mut m = meta(&id);
        m.archived = true;
        write_reading(&lib, m, "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        set_archived(&lib, &conn, &id, false).unwrap();
        assert!(!read_flag(&conn, &id, "archived"));
    }

    // ── favorite ─────────────────────────────────────────────────────────────

    #[test]
    fn set_favorite_updates_frontmatter_and_index() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        set_favorite(&lib, &conn, &id, true).unwrap();

        let content = fs::read_to_string(lib.article_path(&id)).unwrap();
        assert!(content.contains("favorite: true"));
        assert!(read_flag(&conn, &id, "favorite"));
    }

    // ── error handling ────────────────────────────────────────────────────────

    #[test]
    fn returns_error_for_unknown_id() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        assert!(set_read(&lib, &conn, "no-such-id", true).is_err());
        assert!(set_archived(&lib, &conn, "no-such-id", true).is_err());
        assert!(set_favorite(&lib, &conn, "no-such-id", true).is_err());
    }
}
