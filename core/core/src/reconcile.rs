// SPDX-License-Identifier: MIT

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::{
    scanner::{scan_library, ScanDiff, ScannedReading},
    LibraryRoot,
};

/// Apply a set of scanner diffs to the index in a single transaction.
pub fn apply_diffs(conn: &Connection, diffs: &[ScanDiff]) -> Result<()> {
    if diffs.is_empty() {
        return Ok(());
    }

    let tx = conn.unchecked_transaction()?;

    for diff in diffs {
        match diff {
            ScanDiff::Added(r) => insert(conn, r)?,
            ScanDiff::Changed(r) => update(conn, r)?,
            ScanDiff::Removed(id) => delete(conn, id)?,
        }
    }

    tx.commit()?;
    Ok(())
}

/// Rebuild the index from scratch: clear all rows then re-insert every article.
///
/// The FTS triggers keep `readings_fts` in sync automatically.
pub fn rebuild(conn: &Connection, library: &LibraryRoot) -> Result<()> {
    let readings = scan_library(library)?;
    rebuild_scanned(conn, &readings)
}

/// Rebuild from a snapshot the caller already scanned. This lets long-lived
/// clients retain the exact same snapshot for incremental diffing without
/// opening and hashing every preview twice at boot.
pub(crate) fn rebuild_scanned(conn: &Connection, readings: &[ScannedReading]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    conn.execute("DELETE FROM readings", [])?;

    for r in readings {
        insert(conn, r)?;
    }

    tx.commit()?;
    Ok(())
}

fn insert(conn: &Connection, r: &ScannedReading) -> Result<()> {
    let tag_entries = crate::tags::tag_entries_for_reading(
        &r.metadata,
        &r.body,
        r.visual_asset
            .as_ref()
            .map(|asset| asset.content_hash.as_str()),
    );
    let tags: Vec<_> = tag_entries.iter().map(|entry| entry.name.clone()).collect();
    let machine_tags: Vec<_> = tag_entries
        .iter()
        .filter(|entry| entry.origin == "machine")
        .map(|entry| entry.name.clone())
        .collect();
    let tags_json = serde_json::to_string(&tags)?;
    let tag_entries_json = serde_json::to_string(&tag_entries)?;
    let machine_tags_json = serde_json::to_string(&machine_tags)?;
    let tags_text = tags.join(" ");
    let source_profile_json = r
        .metadata
        .source_profile
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let projection = match r.visual_asset.as_ref() {
        Some(asset) => crate::visual_index::cached_projection(conn, &asset.content_hash)?,
        None => Default::default(),
    };
    let card = crate::card_description::project_article_card(
        r.metadata.kind,
        r.metadata.lightweight,
        r.metadata
            .source_profile
            .as_ref()
            .map(|profile| profile.source_type.as_str()),
        &r.metadata.title,
        r.metadata.excerpt.as_deref(),
        r.metadata.word_count,
        &r.body,
    );
    conn.execute(
        "INSERT OR REPLACE INTO readings
         (id, kind, lightweight, has_note, url, media_url, preview_asset, favicon_asset,
          theme_color, canonical_url, title, author, site, saved_at, read_at, archived,
          favorite, rating, source_hash, excerpt, word_count, lang, tags_json, tags_text,
          body_text, visual_asset_path, visual_asset_hash, visual_analyzer_version,
          visual_terms, predominant_color, media_aspect_ratio, source_profile_json,
          card_description, tag_entries_json, machine_tags_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35)",
        params![
            r.metadata.id,
            r.metadata.kind.as_str(),
            r.metadata.lightweight as i32,
            r.has_note as i32,
            r.metadata.url,
            r.metadata.media_url,
            r.metadata.preview_asset,
            r.metadata.favicon_asset,
            r.metadata.theme_color,
            r.metadata.canonical_url,
            r.metadata.title,
            r.metadata.author,
            r.metadata.site,
            r.metadata.saved_at,
            r.metadata.read_at,
            r.metadata.archived as i32,
            r.metadata.favorite as i32,
            r.metadata.rating,
            r.metadata.source_hash,
            r.metadata.excerpt,
            card.word_count,
            r.metadata.lang,
            tags_json,
            tags_text,
            r.body,
            r.visual_asset.as_ref().map(|asset| &asset.relative_path),
            r.visual_asset.as_ref().map(|asset| &asset.content_hash),
            projection.analyzer_version,
            projection.visual_terms,
            projection.predominant_color,
            r.media_aspect_ratio,
            source_profile_json,
            card.description,
            tag_entries_json,
            machine_tags_json,
        ],
    )?;
    Ok(())
}

fn update(conn: &Connection, r: &ScannedReading) -> Result<()> {
    let tag_entries = crate::tags::tag_entries_for_reading(
        &r.metadata,
        &r.body,
        r.visual_asset
            .as_ref()
            .map(|asset| asset.content_hash.as_str()),
    );
    let tags: Vec<_> = tag_entries.iter().map(|entry| entry.name.clone()).collect();
    let machine_tags: Vec<_> = tag_entries
        .iter()
        .filter(|entry| entry.origin == "machine")
        .map(|entry| entry.name.clone())
        .collect();
    let tags_json = serde_json::to_string(&tags)?;
    let tag_entries_json = serde_json::to_string(&tag_entries)?;
    let machine_tags_json = serde_json::to_string(&machine_tags)?;
    let tags_text = tags.join(" ");
    let source_profile_json = r
        .metadata
        .source_profile
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let projection = match r.visual_asset.as_ref() {
        Some(asset) => crate::visual_index::cached_projection(conn, &asset.content_hash)?,
        None => Default::default(),
    };
    let card = crate::card_description::project_article_card(
        r.metadata.kind,
        r.metadata.lightweight,
        r.metadata
            .source_profile
            .as_ref()
            .map(|profile| profile.source_type.as_str()),
        &r.metadata.title,
        r.metadata.excerpt.as_deref(),
        r.metadata.word_count,
        &r.body,
    );
    conn.execute(
        "UPDATE readings SET
         kind=?2, lightweight=?3, has_note=?4, url=?5, media_url=?6, preview_asset=?7,
         favicon_asset=?8, theme_color=?9, canonical_url=?10, title=?11, author=?12,
         site=?13, saved_at=?14, read_at=?15, archived=?16, favorite=?17, rating=?18,
         source_hash=?19, excerpt=?20, word_count=?21, lang=?22, tags_json=?23,
         tags_text=?24, body_text=?25, visual_asset_path=?26, visual_asset_hash=?27,
         visual_analyzer_version=?28, visual_terms=?29, predominant_color=?30,
         media_aspect_ratio=?31, source_profile_json=?32, card_description=?33,
         tag_entries_json=?34, machine_tags_json=?35
         WHERE id=?1",
        params![
            r.metadata.id,
            r.metadata.kind.as_str(),
            r.metadata.lightweight as i32,
            r.has_note as i32,
            r.metadata.url,
            r.metadata.media_url,
            r.metadata.preview_asset,
            r.metadata.favicon_asset,
            r.metadata.theme_color,
            r.metadata.canonical_url,
            r.metadata.title,
            r.metadata.author,
            r.metadata.site,
            r.metadata.saved_at,
            r.metadata.read_at,
            r.metadata.archived as i32,
            r.metadata.favorite as i32,
            r.metadata.rating,
            r.metadata.source_hash,
            r.metadata.excerpt,
            card.word_count,
            r.metadata.lang,
            tags_json,
            tags_text,
            r.body,
            r.visual_asset.as_ref().map(|asset| &asset.relative_path),
            r.visual_asset.as_ref().map(|asset| &asset.content_hash),
            projection.analyzer_version,
            projection.visual_terms,
            projection.predominant_color,
            r.media_aspect_ratio,
            source_profile_json,
            card.description,
            tag_entries_json,
            machine_tags_json,
        ],
    )?;
    Ok(())
}

fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM readings WHERE id=?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::{index::open, new_id, scanner, write_reading, LibraryRoot, Metadata};

    fn make_library(dir: &TempDir) -> LibraryRoot {
        fs::create_dir_all(dir.path().join("articles")).unwrap();
        LibraryRoot::new(dir.path()).unwrap()
    }

    fn sample_meta(id: &str, url: &str) -> Metadata {
        Metadata {
            format_version: 1,
            id: id.to_string(),
            kind: Default::default(),
            lightweight: false,
            url: url.to_string(),
            media_url: None,
            preview_asset: None,
            favicon_asset: None,
            theme_color: None,
            canonical_url: url.to_string(),
            title: "Title".to_string(),
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

    fn row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM readings", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn rebuild_indexes_all_articles() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        write_reading(
            &lib,
            sample_meta(&new_id(), "https://a.com"),
            "body a".to_string(),
        )
        .unwrap();
        write_reading(
            &lib,
            sample_meta(&new_id(), "https://b.com"),
            "body b".to_string(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();
        assert_eq!(row_count(&conn), 2);
    }

    #[test]
    fn rebuild_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        write_reading(
            &lib,
            sample_meta(&new_id(), "https://a.com"),
            "body".to_string(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();
        rebuild(&conn, &lib).unwrap();
        assert_eq!(row_count(&conn), 1);
    }

    #[test]
    fn rebuild_indexes_media_metadata() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();
        let id = new_id();
        let mut metadata = sample_meta(&id, "https://example.com/gallery");
        metadata.kind = crate::ReadingKind::Image;
        metadata.media_url = Some("https://cdn.example.com/photo.jpg".into());
        metadata.preview_asset = Some("assets/photo.png".into());
        write_reading(&lib, metadata, "![Photo](assets/photo.png)".into()).unwrap();
        let mut png = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        png.extend_from_slice(&1900_u32.to_be_bytes());
        png.extend_from_slice(&2468_u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        fs::create_dir_all(lib.assets_dir(&id)).unwrap();
        fs::write(lib.assets_dir(&id).join("photo.png"), png).unwrap();

        rebuild(&conn, &lib).unwrap();

        let values: (String, Option<String>, Option<String>, Option<f64>) = conn
            .query_row(
                "SELECT kind, media_url, preview_asset, media_aspect_ratio
                 FROM readings WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            values,
            (
                "image".into(),
                Some("https://cdn.example.com/photo.jpg".into()),
                Some("assets/photo.png".into()),
                Some(1900.0 / 2468.0)
            )
        );
    }

    #[test]
    fn apply_diffs_add_inserts_row() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let id = new_id();
        write_reading(
            &lib,
            sample_meta(&id, "https://example.com"),
            "hello".to_string(),
        )
        .unwrap();

        let new_scan = scanner::scan_library(&lib).unwrap();
        let diffs = scanner::diff(&[], &new_scan);
        apply_diffs(&conn, &diffs).unwrap();

        assert_eq!(row_count(&conn), 1);
    }

    #[test]
    fn apply_diffs_remove_deletes_row() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let id = new_id();
        write_reading(
            &lib,
            sample_meta(&id, "https://example.com"),
            "hello".to_string(),
        )
        .unwrap();
        let old_scan = scanner::scan_library(&lib).unwrap();
        apply_diffs(&conn, &scanner::diff(&[], &old_scan)).unwrap();

        fs::remove_file(lib.article_path(&id)).unwrap();
        let new_scan = scanner::scan_library(&lib).unwrap();
        let diffs = scanner::diff(&old_scan, &new_scan);
        apply_diffs(&conn, &diffs).unwrap();

        assert_eq!(row_count(&conn), 0);
    }

    #[test]
    fn apply_diffs_change_updates_row() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let id = new_id();
        write_reading(
            &lib,
            sample_meta(&id, "https://example.com"),
            "v1".to_string(),
        )
        .unwrap();
        let old_scan = scanner::scan_library(&lib).unwrap();
        apply_diffs(&conn, &scanner::diff(&[], &old_scan)).unwrap();

        write_reading(
            &lib,
            sample_meta(&id, "https://example.com"),
            "v2".to_string(),
        )
        .unwrap();
        let new_scan = scanner::scan_library(&lib).unwrap();
        let diffs = scanner::diff(&old_scan, &new_scan);
        apply_diffs(&conn, &diffs).unwrap();

        // Still one row, but body changed.
        assert_eq!(row_count(&conn), 1);
        let body: String = conn
            .query_row(
                "SELECT body_text FROM readings WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(body.contains("v2"));
    }

    #[test]
    fn rebuild_then_diff_no_ops_are_empty() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        write_reading(
            &lib,
            sample_meta(&new_id(), "https://a.com"),
            "body".to_string(),
        )
        .unwrap();

        let scan = scanner::scan_library(&lib).unwrap();
        rebuild(&conn, &lib).unwrap();

        // Diff a scan against itself — nothing to apply.
        let diffs = scanner::diff(&scan, &scan);
        assert!(diffs.is_empty());
        apply_diffs(&conn, &diffs).unwrap();
        assert_eq!(row_count(&conn), 1);
    }
}
