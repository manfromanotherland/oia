// SPDX-License-Identifier: MIT

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

/// Open (or create) the index database at `db_path` and run any pending migrations.
pub fn open(db_path: &Path) -> Result<Connection> {
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    if std::env::var_os("SQL_TRACE").is_some() {
        install_sql_trace(&mut conn);
    }
    Ok(conn)
}

/// Log every executed SQL statement (with its wall-clock duration) to stderr.
///
/// Gated on the `SQL_TRACE` env var so release builds pay nothing. Useful for
/// spotting chatty callers and N+1 patterns — a statement that repeats dozens
/// of times per UI action shows up as an obvious run of identical lines.
fn install_sql_trace(conn: &mut Connection) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    conn.profile(Some(|sql: &str, dur: std::time::Duration| {
        let n = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let ms = dur.as_secs_f64() * 1e3;
        if ms < 1.0 {
            eprintln!("[sql #{n} {:>6.2}µs] {sql}", ms * 1e3);
        } else {
            eprintln!("[sql #{n} {:>6.2}ms] {sql}", ms);
        }
    }));
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 1 {
        migrate_v1(conn)?;
    }
    if version < 2 {
        migrate_v2(conn)?;
    }
    if version < 3 {
        migrate_v3(conn)?;
    }
    if version < 4 {
        migrate_v4(conn)?;
    }
    if version < 5 {
        migrate_v5(conn)?;
    }
    if version < 6 {
        migrate_v6(conn)?;
    }
    if version < 7 {
        migrate_v7(conn)?;
    }
    if version < 8 {
        migrate_v8(conn)?;
    }
    if version < 9 {
        migrate_v9(conn)?;
    }
    if version < 10 {
        migrate_v10(conn)?;
    }
    Ok(())
}

/// v10: cache bounded descriptions for full-article cards. The projection can
/// always be rebuilt from `body_text` and frontmatter metadata; no library file
/// is changed. Existing rows are backfilled in small batches on upgrade.
fn migrate_v10(conn: &Connection) -> Result<()> {
    conn.execute_batch("BEGIN; ALTER TABLE readings ADD COLUMN card_description TEXT;")?;
    let backfill = (|| -> Result<()> {
        let mut after_id = String::new();
        loop {
            let batch = {
                let mut stmt = conn.prepare(
                    "SELECT id, title, excerpt, word_count, body_text, source_profile_json
                     FROM readings
                     WHERE id > ?1 AND kind = 'article' AND lightweight = 0
                     ORDER BY id LIMIT 8",
                )?;
                let rows = stmt.query_map([&after_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<u32>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            if batch.is_empty() {
                break;
            }
            for (id, title, excerpt, word_count, body, profile_json) in &batch {
                let source_type = profile_json
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                    .and_then(|profile| {
                        profile
                            .get("source_type")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    });
                let card = crate::card_description::project_article_card(
                    crate::ReadingKind::Article,
                    false,
                    source_type.as_deref(),
                    title,
                    excerpt.as_deref(),
                    *word_count,
                    body,
                );
                conn.execute(
                    "UPDATE readings SET card_description=?2, word_count=?3 WHERE id=?1",
                    rusqlite::params![id, card.description, card.word_count],
                )?;
            }
            after_id = batch.last().expect("nonempty batch").0.clone();
        }
        Ok(())
    })();
    match backfill {
        Ok(()) => conn.execute_batch("PRAGMA user_version = 10; COMMIT;")?,
        Err(error) => {
            conn.execute_batch("ROLLBACK;")?;
            return Err(error);
        }
    }
    Ok(())
}

/// Pending analysis deduplicates readings by asset hash. Bound those lookups
/// (and result fan-out updates) by hash rather than searching all reading IDs.
fn migrate_v9(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "BEGIN;
         CREATE INDEX readings_visual_asset_hash_id_idx ON readings(visual_asset_hash, id);
         PRAGMA user_version = 9;
         COMMIT;",
    )?;
    Ok(())
}

/// v8: cache the provider-neutral source profile as JSON for list clients.
///
/// The typed frontmatter remains authoritative. This nullable projection is
/// intentionally opaque to SQLite so open string discriminators survive from
/// scanner to clients without a schema migration per provider or attachment
/// kind.
fn migrate_v8(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        BEGIN;

        ALTER TABLE readings ADD COLUMN source_profile_json TEXT;

        PRAGMA user_version = 8;
        COMMIT;
        ",
    )?;
    Ok(())
}

/// v7: serve the board's fixed newest-first order directly from the index.
///
/// The macOS board loads one complete, immutable snapshot for LazyLayoutKit.
/// Keeping that snapshot in `saved_at DESC, id DESC` order avoids building a
/// temporary SQLite sort tree on every cached launch, refresh, and scope change.
fn migrate_v7(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        BEGIN;

        CREATE INDEX readings_saved_at_id_idx
            ON readings(saved_at DESC, id DESC);

        PRAGMA user_version = 7;
        COMMIT;
        ",
    )?;
    Ok(())
}

/// v6: cache display-oriented media geometry for deterministic masonry frames.
///
/// The ratio is derived from local asset headers during reconciliation and can
/// always be rebuilt; it is not part of the synced library contract.
fn migrate_v6(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        BEGIN;

        ALTER TABLE readings ADD COLUMN media_aspect_ratio REAL
            CHECK (media_aspect_ratio IS NULL OR media_aspect_ratio > 0);

        PRAGMA user_version = 6;
        COMMIT;
        ",
    )?;
    Ok(())
}

/// v5: expose locally captured favicons and website theme colors to clients.
///
/// Both columns are disposable projections of optional frontmatter fields.
fn migrate_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        BEGIN;

        ALTER TABLE readings ADD COLUMN favicon_asset TEXT;
        ALTER TABLE readings ADD COLUMN theme_color TEXT;

        PRAGMA user_version = 5;
        COMMIT;
        ",
    )?;
    Ok(())
}

/// v4: cache device-local visual analysis and make its derived terms searchable.
///
/// `visual_analysis` is deliberately independent of `readings`: rebuilding the
/// disposable readings projection must not throw away expensive analysis for
/// bytes that are still present (or later reappear). The composite key also
/// makes analyzer upgrades explicit instead of silently mixing vocabularies.
fn migrate_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        BEGIN;

        ALTER TABLE readings ADD COLUMN visual_asset_path TEXT;
        ALTER TABLE readings ADD COLUMN visual_asset_hash TEXT;
        ALTER TABLE readings ADD COLUMN visual_analyzer_version TEXT;
        ALTER TABLE readings ADD COLUMN visual_terms TEXT NOT NULL DEFAULT '';
        ALTER TABLE readings ADD COLUMN predominant_color TEXT;

        CREATE TABLE visual_analysis (
            content_hash      TEXT NOT NULL,
            analyzer_version  TEXT NOT NULL,
            supported         INTEGER NOT NULL CHECK (supported IN (0, 1)),
            labels_json       TEXT NOT NULL DEFAULT '[]',
            palette_json      TEXT NOT NULL DEFAULT '[]',
            visual_terms      TEXT NOT NULL DEFAULT '',
            predominant_color TEXT,
            completed_at      INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (content_hash, analyzer_version)
        );

        CREATE INDEX visual_analysis_hash_idx
            ON visual_analysis(content_hash);

        DROP TRIGGER IF EXISTS readings_ai;
        DROP TRIGGER IF EXISTS readings_ad;
        DROP TRIGGER IF EXISTS readings_au;
        DROP TABLE readings_fts;

        CREATE VIRTUAL TABLE readings_fts USING fts5(
            title,
            body_text,
            site,
            tags_text,
            visual_terms,
            content=readings,
            content_rowid=rowid
        );

        CREATE TRIGGER readings_ai
        AFTER INSERT ON readings BEGIN
            INSERT INTO readings_fts(rowid, title, body_text, site, tags_text, visual_terms)
            VALUES (new.rowid, new.title, new.body_text, new.site, new.tags_text,
                    new.visual_terms);
        END;

        CREATE TRIGGER readings_ad
        AFTER DELETE ON readings BEGIN
            INSERT INTO readings_fts(readings_fts, rowid, title, body_text, site, tags_text,
                                     visual_terms)
            VALUES ('delete', old.rowid, old.title, old.body_text, old.site, old.tags_text,
                    old.visual_terms);
        END;

        CREATE TRIGGER readings_au
        AFTER UPDATE ON readings BEGIN
            INSERT INTO readings_fts(readings_fts, rowid, title, body_text, site, tags_text,
                                     visual_terms)
            VALUES ('delete', old.rowid, old.title, old.body_text, old.site, old.tags_text,
                    old.visual_terms);
            INSERT INTO readings_fts(rowid, title, body_text, site, tags_text, visual_terms)
            VALUES (new.rowid, new.title, new.body_text, new.site, new.tags_text,
                    new.visual_terms);
        END;

        INSERT INTO readings_fts(readings_fts) VALUES ('rebuild');

        PRAGMA user_version = 4;
        COMMIT;
        ",
    )?;
    Ok(())
}

/// v1: the initial schema.
///
/// This is a fresh baseline: the app has no released users, so the earlier
/// incremental migrations were collapsed into this single step rather than
/// carried forever. It creates the `readings` table, the `readings_fts`
/// full-text index (covering title, body, and site), and the triggers that keep
/// the two in sync. Later releases add migrations on top of this — see
/// [`migrate`].
fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        BEGIN;

        CREATE TABLE IF NOT EXISTS readings (
            id            TEXT    PRIMARY KEY NOT NULL,
            url           TEXT    NOT NULL,
            canonical_url TEXT    NOT NULL,
            title         TEXT    NOT NULL,
            author        TEXT,
            site          TEXT,
            saved_at      TEXT    NOT NULL,
            read_at       TEXT,
            archived      INTEGER NOT NULL DEFAULT 0,
            favorite      INTEGER NOT NULL DEFAULT 0,
            rating        INTEGER NOT NULL DEFAULT 0,
            source_hash   TEXT    NOT NULL,
            excerpt       TEXT,
            word_count    INTEGER,
            lang          TEXT,
            tags_json     TEXT    NOT NULL DEFAULT '[]',
            body_text     TEXT    NOT NULL DEFAULT ''
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS readings_fts USING fts5(
            title,
            body_text,
            site,
            content=readings,
            content_rowid=rowid
        );

        -- Keep FTS in sync with the readings table.
        CREATE TRIGGER IF NOT EXISTS readings_ai
        AFTER INSERT ON readings BEGIN
            INSERT INTO readings_fts(rowid, title, body_text, site)
            VALUES (new.rowid, new.title, new.body_text, new.site);
        END;

        CREATE TRIGGER IF NOT EXISTS readings_ad
        AFTER DELETE ON readings BEGIN
            INSERT INTO readings_fts(readings_fts, rowid, title, body_text, site)
            VALUES ('delete', old.rowid, old.title, old.body_text, old.site);
        END;

        CREATE TRIGGER IF NOT EXISTS readings_au
        AFTER UPDATE ON readings BEGIN
            INSERT INTO readings_fts(readings_fts, rowid, title, body_text, site)
            VALUES ('delete', old.rowid, old.title, old.body_text, old.site);
            INSERT INTO readings_fts(rowid, title, body_text, site)
            VALUES (new.rowid, new.title, new.body_text, new.site);
        END;

        PRAGMA user_version = 1;

        COMMIT;
        ",
    )?;
    Ok(())
}

/// v2: distinguish articles, images, videos, and quotes and expose local previews.
///
/// Existing rows become articles; media and preview paths remain absent. The
/// columns are derived from frontmatter and therefore remain disposable cache
/// data like the rest of the index.
fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        BEGIN;

        ALTER TABLE readings
            ADD COLUMN kind TEXT NOT NULL DEFAULT 'article'
            CHECK (kind IN ('article', 'image', 'video', 'quote'));
        ALTER TABLE readings ADD COLUMN media_url TEXT;
        ALTER TABLE readings ADD COLUMN preview_asset TEXT;

        PRAGMA user_version = 2;

        COMMIT;
        ",
    )?;
    Ok(())
}

/// v3: index lightweight links, personal-note presence, and searchable tags.
///
/// All three columns remain disposable cache data derived from the reading
/// folder. Recreate the external-content FTS table so tag text participates in
/// the same title/body/site search without changing the Markdown contract.
fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        BEGIN;

        ALTER TABLE readings ADD COLUMN lightweight INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE readings ADD COLUMN has_note INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE readings ADD COLUMN tags_text TEXT NOT NULL DEFAULT '';

        UPDATE readings
        SET tags_text = COALESCE(
            (SELECT group_concat(value, ' ') FROM json_each(readings.tags_json)),
            ''
        );

        DROP TRIGGER IF EXISTS readings_ai;
        DROP TRIGGER IF EXISTS readings_ad;
        DROP TRIGGER IF EXISTS readings_au;
        DROP TABLE readings_fts;

        CREATE VIRTUAL TABLE readings_fts USING fts5(
            title,
            body_text,
            site,
            tags_text,
            content=readings,
            content_rowid=rowid
        );

        CREATE TRIGGER readings_ai
        AFTER INSERT ON readings BEGIN
            INSERT INTO readings_fts(rowid, title, body_text, site, tags_text)
            VALUES (new.rowid, new.title, new.body_text, new.site, new.tags_text);
        END;

        CREATE TRIGGER readings_ad
        AFTER DELETE ON readings BEGIN
            INSERT INTO readings_fts(readings_fts, rowid, title, body_text, site, tags_text)
            VALUES ('delete', old.rowid, old.title, old.body_text, old.site, old.tags_text);
        END;

        CREATE TRIGGER readings_au
        AFTER UPDATE ON readings BEGIN
            INSERT INTO readings_fts(readings_fts, rowid, title, body_text, site, tags_text)
            VALUES ('delete', old.rowid, old.title, old.body_text, old.site, old.tags_text);
            INSERT INTO readings_fts(rowid, title, body_text, site, tags_text)
            VALUES (new.rowid, new.title, new.body_text, new.site, new.tags_text);
        END;

        INSERT INTO readings_fts(readings_fts) VALUES ('rebuild');

        PRAGMA user_version = 3;

        COMMIT;
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_temp() -> (TempDir, Connection) {
        let dir = TempDir::new().unwrap();
        let conn = open(&dir.path().join("index.db")).unwrap();
        (dir, conn)
    }

    #[test]
    fn creates_schema_on_fresh_db() {
        let (_dir, conn) = open_temp();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 10);

        // readings table exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM readings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn fts_table_exists() {
        let (_dir, conn) = open_temp();
        // FTS5 tables show up in sqlite_master.
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='readings_fts'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap();
        assert!(exists, "readings_fts table should exist");
    }

    #[test]
    fn fts_indexes_site() {
        let (_dir, conn) = open_temp();

        conn.execute(
            "INSERT INTO readings
             (id, url, canonical_url, title, site, saved_at, source_hash, body_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                "01TESTID",
                "https://nytimes.com",
                "https://nytimes.com",
                "Headline",
                "nytimes.com",
                "2026-06-13T15:00:00Z",
                "sha256:abc",
                "Body without the search term."
            ],
        )
        .unwrap();

        // The term lives only in the site column, yet FTS finds it.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM readings_fts WHERE readings_fts MATCH 'nytimes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "site token should be searchable via FTS");
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");

        // Open twice — second open should not fail.
        open(&db_path).unwrap();
        open(&db_path).unwrap();
    }

    #[test]
    fn visual_asset_identity_lookup_is_bounded_by_the_content_hash() {
        let directory = TempDir::new().unwrap();
        let connection = open(&directory.path().join("index.db")).unwrap();
        let mut statement = connection
            .prepare(
                "EXPLAIN QUERY PLAN SELECT MIN(id) FROM readings
             WHERE visual_asset_hash=?1 AND visual_asset_path IS NOT NULL",
            )
            .unwrap();
        let plan: Vec<String> = statement
            .query_map(["same-image"], |row| row.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(plan.iter().any(|detail| detail.contains("visual_asset_hash=?")),
            "candidate deduplication must seek by hash instead of searching unrelated readings: {plan:?}");
    }

    #[test]
    fn v7_indexes_existing_readings_without_changing_board_order() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            migrate_v1(&conn).unwrap();
            migrate_v2(&conn).unwrap();
            migrate_v3(&conn).unwrap();
            migrate_v4(&conn).unwrap();
            migrate_v5(&conn).unwrap();
            migrate_v6(&conn).unwrap();
            for (id, saved_at) in [
                ("older", "2026-06-13T15:00:00Z"),
                ("newer", "2026-06-14T15:00:00Z"),
            ] {
                conn.execute(
                    "INSERT INTO readings
                     (id, url, canonical_url, title, saved_at, source_hash)
                     VALUES (?1, ?2, ?2, ?1, ?3, ?1)",
                    rusqlite::params![id, format!("https://example.com/{id}"), saved_at],
                )
                .unwrap();
            }
        }

        let conn = open(&db_path).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 10);

        let mut stmt = conn
            .prepare("SELECT id FROM readings ORDER BY saved_at DESC, id DESC")
            .unwrap();
        let ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(ids, vec!["newer", "older"]);

        let mut plan = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT id FROM readings ORDER BY saved_at DESC, id DESC",
            )
            .unwrap();
        let details: Vec<String> = plan
            .query_map([], |row| row.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("readings_saved_at_id_idx")),
            "board query should use the saved-order index: {details:?}"
        );
    }

    #[test]
    fn insert_and_fts_search() {
        let (_dir, conn) = open_temp();

        conn.execute(
            "INSERT INTO readings
             (id, url, canonical_url, title, saved_at, source_hash, body_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "01TESTID",
                "https://example.com",
                "https://example.com",
                "Rust programming",
                "2026-06-13T15:00:00Z",
                "sha256:abc",
                "Rust is a systems programming language."
            ],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM readings_fts WHERE readings_fts MATCH 'rust'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "FTS search for 'rust' should find the inserted row"
        );
    }

    #[test]
    fn schema_has_expected_columns() {
        let (_dir, conn) = open_temp();

        // PRAGMA table_info returns one row per column.
        let mut stmt = conn.prepare("PRAGMA table_info(readings)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        for col in &[
            "id",
            "kind",
            "lightweight",
            "has_note",
            "url",
            "media_url",
            "preview_asset",
            "favicon_asset",
            "theme_color",
            "canonical_url",
            "title",
            "read_at",
            "archived",
            "favorite",
            "rating",
            "source_hash",
            "tags_json",
            "tags_text",
            "body_text",
            "visual_asset_path",
            "visual_asset_hash",
            "visual_analyzer_version",
            "visual_terms",
            "predominant_color",
            "media_aspect_ratio",
            "source_profile_json",
            "card_description",
        ] {
            assert!(columns.contains(&col.to_string()), "missing column: {col}");
        }

        // The old `read` boolean is gone — read state lives in `read_at`.
        assert!(
            !columns.contains(&"read".to_string()),
            "legacy `read` column should be dropped"
        );
    }

    #[test]
    fn media_aspect_ratio_accepts_only_positive_values() {
        let (_dir, conn) = open_temp();

        let result = conn.execute(
            "INSERT INTO readings
             (id, url, canonical_url, title, saved_at, source_hash, media_aspect_ratio)
             VALUES ('ratio', 'https://example.com', 'https://example.com', 'Ratio',
                     '2026-06-13T15:00:00Z', 'sha256:ratio', -1)",
            [],
        );

        assert!(result.is_err());
    }

    #[test]
    fn v8_adds_a_nullable_source_profile_projection_to_existing_rows() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            migrate_v1(&conn).unwrap();
            migrate_v2(&conn).unwrap();
            migrate_v3(&conn).unwrap();
            migrate_v4(&conn).unwrap();
            migrate_v5(&conn).unwrap();
            migrate_v6(&conn).unwrap();
            migrate_v7(&conn).unwrap();
            conn.execute(
                "INSERT INTO readings
                 (id, url, canonical_url, title, saved_at, source_hash)
                 VALUES ('existing', 'https://example.com/post', 'https://example.com/post',
                         'Existing', '2026-09-22T18:42:00.000Z', 'sha256:existing')",
                [],
            )
            .unwrap();
        }

        let conn = open(&db_path).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        let projected: Option<String> = conn
            .query_row(
                "SELECT source_profile_json FROM readings WHERE id = 'existing'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, 10);
        assert_eq!(projected, None);
    }

    #[test]
    fn v10_backfills_article_cards_without_changing_saved_excerpts() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            migrate_v1(&conn).unwrap();
            migrate_v2(&conn).unwrap();
            migrate_v3(&conn).unwrap();
            migrate_v4(&conn).unwrap();
            migrate_v5(&conn).unwrap();
            migrate_v6(&conn).unwrap();
            migrate_v7(&conn).unwrap();
            migrate_v8(&conn).unwrap();
            migrate_v9(&conn).unwrap();
            for (id, title, excerpt, body, lightweight, source_profile_json) in [
                (
                    "article",
                    "A heading",
                    Some("A heading."),
                    "# A heading\n\nThe saved body starts here.",
                    0,
                    None,
                ),
                (
                    "link",
                    "Link",
                    None,
                    "[Open link](https://example.com)",
                    1,
                    None,
                ),
                (
                    "social",
                    "Social",
                    None,
                    "The post text",
                    0,
                    Some(r#"{"source_type":"social_post"}"#),
                ),
            ] {
                conn.execute(
                    "INSERT INTO readings
                     (id, url, canonical_url, title, saved_at, source_hash,
                      excerpt, body_text, lightweight, source_profile_json)
                     VALUES (?1, 'https://example.com', 'https://example.com', ?2,
                             '2026-09-22T18:42:00.000Z', 'sha256:existing', ?3, ?4, ?5, ?6)",
                    rusqlite::params![id, title, excerpt, body, lightweight, source_profile_json],
                )
                .unwrap();
            }
        }

        let conn = open(&db_path).unwrap();
        let article: (Option<String>, Option<String>, Option<u32>) = conn
            .query_row(
                "SELECT excerpt, card_description, word_count FROM readings WHERE id='article'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(article.0.as_deref(), Some("A heading."));
        assert_eq!(article.1.as_deref(), Some("The saved body starts here."));
        assert_eq!(article.2, Some(7));
        for id in ["link", "social"] {
            let card: (Option<String>, Option<u32>) = conn
                .query_row(
                    "SELECT card_description, word_count FROM readings WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(card, (None, None));
        }
    }

    #[test]
    fn v2_migrates_existing_rows_to_article_defaults() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");

        {
            let conn = Connection::open(&db_path).unwrap();
            migrate_v1(&conn).unwrap();
            conn.execute(
                "INSERT INTO readings
                 (id, url, canonical_url, title, saved_at, source_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "legacy-id",
                    "https://example.com/legacy",
                    "https://example.com/legacy",
                    "Legacy",
                    "2026-06-13T15:00:00Z",
                    "sha256:abc"
                ],
            )
            .unwrap();
        }

        let conn = open(&db_path).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 10);

        let values: (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT kind, media_url, preview_asset FROM readings WHERE id = 'legacy-id'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(values, ("article".into(), None, None));
    }

    #[test]
    fn v3_derives_searchable_tags_and_filter_flags_for_existing_rows() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");

        {
            let conn = Connection::open(&db_path).unwrap();
            migrate_v1(&conn).unwrap();
            migrate_v2(&conn).unwrap();
            conn.execute(
                "INSERT INTO readings
                 (id, url, canonical_url, title, saved_at, source_hash, tags_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    "existing-id",
                    "https://example.com/existing",
                    "https://example.com/existing",
                    "Existing",
                    "2026-06-13T15:00:00Z",
                    "sha256:abc",
                    r#"["local-first","design"]"#
                ],
            )
            .unwrap();
        }

        let conn = open(&db_path).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 10);

        let values: (i64, i64, String) = conn
            .query_row(
                "SELECT lightweight, has_note, tags_text FROM readings WHERE id = 'existing-id'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(values, (0, 0, "local-first design".into()));

        let matches: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM readings_fts WHERE readings_fts MATCH 'local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(matches, 1);
    }
}
