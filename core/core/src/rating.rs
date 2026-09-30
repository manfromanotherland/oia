// SPDX-License-Identifier: MIT

//! Star ratings (1–5, or 0 for unrated).
//!
//! A rating lives in the article's frontmatter (`rating:`) as the source of
//! truth and is mirrored into the `rating` index column. `list_ratings`
//! returns the per-value counts that drive the sidebar's Ratings filter.

use anyhow::{bail, Result};
use rusqlite::{params_from_iter, types::Value, Connection};

use crate::{
    list::{pinned_count_filter, CountScope, Facet, ResolvedSearch},
    status::update_flag,
    LibraryRoot,
};

/// Set a reading's star rating. `rating` must be 0–5, where 0 clears it.
pub fn set_rating(library: &LibraryRoot, conn: &Connection, id: &str, rating: u8) -> Result<()> {
    if rating > 5 {
        bail!("rating must be 0..=5, got {rating}");
    }
    update_flag(library, conn, id, |m| m.rating = rating)
}

/// The star buckets (1–5) to show in the sidebar's Ratings section, each with
/// its badge count, ordered highest rating first. Mirrors `list_tags`.
///
/// Like the smart-view rows, which are a fixed set: *every* star bucket in use in
/// the library is always present, so switching view, searching, or picking a tag
/// only changes the *badge*, zeroing it rather than hiding the row. This is the
/// Ratings facet, so `scope`'s own rating selection is ignored (a facet never
/// filters itself).
///
/// The count re-applies the full view, the search, and the selected tag through a
/// `COUNT(...) FILTER`. With the default scope (`view = All`) a bucket used only by
/// archived readings shows a 0, and every other bucket its plain non-archived
/// count.
pub fn list_ratings(conn: &Connection, scope: &CountScope) -> Result<Vec<(u8, u64)>> {
    let search = ResolvedSearch::resolve_scoped(conn, scope)?;
    list_ratings_with(conn, scope, &search)
}

/// [`list_ratings`] with the search pre-resolved, so
/// [`crate::list::sidebar_counts`] can share one resolution across all three
/// sidebar sections.
pub(crate) fn list_ratings_with(
    conn: &Connection,
    scope: &CountScope,
    search: &ResolvedSearch,
) -> Result<Vec<(u8, u64)>> {
    // Presence = every star bucket used in the library (fixed set). The badge
    // narrows to the exact view plus the search and sibling tag facet.
    let mut vals: Vec<Value> = Vec::new();
    let count_filter = pinned_count_filter(scope, Facet::Tag, search, &mut vals);

    let sql = format!(
        "SELECT rating, COUNT(*) FILTER (WHERE {count_filter}) AS cnt
         FROM readings
         WHERE rating BETWEEN 1 AND 5
         GROUP BY rating
         ORDER BY rating DESC"
    );
    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(params_from_iter(vals), |row| {
        Ok((row.get::<_, i64>(0)? as u8, row.get::<_, u64>(1)?))
    })?;

    rows.map(|r| r.map_err(Into::into)).collect()
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

    fn rating_of(conn: &Connection, id: &str) -> i64 {
        conn.query_row(
            "SELECT rating FROM readings WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn set_rating_updates_frontmatter_and_index() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();
        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        set_rating(&lib, &conn, &id, 4).unwrap();

        let content = fs::read_to_string(lib.article_path(&id)).unwrap();
        assert!(content.contains("rating: 4"));
        assert_eq!(rating_of(&conn, &id), 4);
    }

    #[test]
    fn set_rating_zero_clears() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();
        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        set_rating(&lib, &conn, &id, 3).unwrap();
        set_rating(&lib, &conn, &id, 0).unwrap();
        assert_eq!(rating_of(&conn, &id), 0);
    }

    #[test]
    fn set_rating_rejects_out_of_range() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();
        let id = new_id();
        write_reading(&lib, meta(&id), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        assert!(set_rating(&lib, &conn, &id, 6).is_err());
    }

    #[test]
    fn list_ratings_counts_and_excludes_archived() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let a = new_id();
        let b = new_id();
        let c = new_id();
        write_reading(&lib, meta(&a), "a".into()).unwrap();
        write_reading(&lib, meta(&b), "b".into()).unwrap();
        write_reading(&lib, meta(&c), "c".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        set_rating(&lib, &conn, &a, 5).unwrap();
        set_rating(&lib, &conn, &b, 5).unwrap();
        set_rating(&lib, &conn, &c, 3).unwrap();

        let ratings = list_ratings(&conn, &CountScope::default()).unwrap();
        assert_eq!(ratings, vec![(5, 2), (3, 1)]);
    }

    /// Build the faceting corpus shared by the scoped rating-count tests:
    /// A (active, 5★, sci, "alpha"), B (active, 5★, cook, "beta"),
    /// C (active, 3★, sci, "gamma"), D (archived, 5★, sci, "delta").
    fn faceting_corpus(lib: &LibraryRoot, conn: &Connection) {
        for (url, rating, tag, archived, body) in [
            ("https://a.com", 5u8, "sci", false, "alpha"),
            ("https://b.com", 5, "cook", false, "beta"),
            ("https://c.com", 3, "sci", false, "gamma"),
            ("https://d.com", 5, "sci", true, "delta"),
        ] {
            let mut m = meta(&new_id());
            m.url = url.into();
            m.canonical_url = url.into();
            m.rating = rating;
            m.tags = vec![tag.into()];
            m.archived = archived;
            write_reading(lib, m, body.into()).unwrap();
        }
        rebuild(conn, lib).unwrap();
    }

    #[test]
    fn list_ratings_scoped_by_search() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();
        faceting_corpus(&lib, &conn);

        // "alpha" is only A (5★). The search scopes the *counts*, not the visible
        // buckets: the 5★ bucket reports 1, while the 3★ bucket (C, "gamma", which
        // doesn't match) stays pinned at 0 rather than disappearing.
        let ratings = list_ratings(
            &conn,
            &CountScope {
                query: Some("alpha".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(ratings, vec![(5, 1), (3, 0)]);
    }

    #[test]
    fn list_ratings_scoped_by_tag_facet() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();
        faceting_corpus(&lib, &conn);

        // The "sci" tag covers A and legacy-archived D (5★), plus C (3★);
        // B is "cook".
        let ratings = list_ratings(
            &conn,
            &CountScope {
                tag: Some("sci".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(ratings, vec![(5, 2), (3, 1)]);
    }

    #[test]
    fn list_ratings_pins_zero_count_buckets_under_a_tag_facet() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        // A: sci, 5★. B: cook, 3★. Both non-archived.
        let mut a = meta(&new_id());
        a.tags = vec!["sci".into()];
        a.rating = 5;
        write_reading(&lib, a, "alpha".into()).unwrap();
        let mut b = meta(&new_id());
        b.url = "https://b.com".into();
        b.canonical_url = "https://b.com".into();
        b.tags = vec!["cook".into()];
        b.rating = 3;
        write_reading(&lib, b, "beta".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // Selecting the "sci" tag keeps BOTH buckets visible (presence ignores the
        // tag facet); the 3★ bucket reports 0 instead of vanishing, since its only
        // reading is tagged "cook".
        let ratings = list_ratings(
            &conn,
            &CountScope {
                tag: Some("sci".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(ratings, vec![(5, 1), (3, 0)]);
    }

    #[test]
    fn list_ratings_pins_all_under_search_plus_tag_selection() {
        // The reported case: a tag is selected AND a search is typed. Every rating
        // bucket in the view must stay visible; only the badges reflect search∩tag.
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let mut a = meta(&new_id()); // scifi, 5★, matches "starship"
        a.tags = vec!["scifi".into()];
        a.rating = 5;
        write_reading(&lib, a, "a lone starship".into()).unwrap();
        let mut b = meta(&new_id()); // space, 3★, matches "starship" but not tag
        b.url = "https://b.com".into();
        b.canonical_url = "https://b.com".into();
        b.tags = vec!["space".into()];
        b.rating = 3;
        write_reading(&lib, b, "a docking starship".into()).unwrap();
        let mut c = meta(&new_id()); // cooking, 2★, no "starship", not tag
        c.url = "https://c.com".into();
        c.canonical_url = "https://c.com".into();
        c.tags = vec!["cooking".into()];
        c.rating = 2;
        write_reading(&lib, c, "today I cooked pasta".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let ratings = list_ratings(
            &conn,
            &CountScope {
                tag: Some("scifi".into()),
                query: Some("starship".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // All three buckets stay; only ★5 (A: scifi ∩ starship) counts, ★3 and ★2
        // are pinned at 0.
        assert_eq!(ratings, vec![(5, 1), (3, 0), (2, 0)]);
    }

    #[test]
    fn list_ratings_pins_across_the_unread_view() {
        // The reported case: searching, then clicking Unread must not drop rating
        // buckets. A bucket whose only reading is *read* stays visible at 0, since
        // All/Unread/Read share one presence pool.
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let mut a = meta(&new_id()); // unread, 5★
        a.rating = 5;
        write_reading(&lib, a, "coding in rust".into()).unwrap();
        let mut b = meta(&new_id()); // read, 3★
        b.url = "https://b.com".into();
        b.canonical_url = "https://b.com".into();
        b.rating = 3;
        b.read_at = Some("2026-06-13T16:00:00.000Z".into());
        write_reading(&lib, b, "coding in swift".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let ratings = list_ratings(
            &conn,
            &CountScope {
                view: crate::list::View::Unread,
                query: Some("coding".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // The ★3 bucket (on the read article) is pinned at 0 rather than hidden.
        assert_eq!(ratings, vec![(5, 1), (3, 0)]);
    }

    #[test]
    fn list_ratings_pins_across_the_favorites_view_without_search() {
        // The reported case: selecting Favorites (no search) must keep every star
        // bucket. A bucket on a non-favorite reading stays visible at 0.
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();

        let mut a = meta(&new_id()); // favorite, 5★
        a.rating = 5;
        a.favorite = true;
        write_reading(&lib, a, "body".into()).unwrap();
        let mut b = meta(&new_id()); // not favorite, 3★
        b.url = "https://b.com".into();
        b.canonical_url = "https://b.com".into();
        b.rating = 3;
        write_reading(&lib, b, "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let ratings = list_ratings(
            &conn,
            &CountScope {
                view: crate::list::View::Favorites,
                ..Default::default()
            },
        )
        .unwrap();
        // The ★3 bucket (on the non-favorite reading) is pinned at 0, not hidden.
        assert_eq!(ratings, vec![(5, 1), (3, 0)]);
    }

    #[test]
    fn list_ratings_ignores_its_own_rating_selection() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let conn = open(&dir.path().join("index.db")).unwrap();
        faceting_corpus(&lib, &conn);

        // A selected rating must NOT filter the rating list — every sibling
        // bucket stays visible — so it matches the default listing.
        let selected = list_ratings(
            &conn,
            &CountScope {
                rating: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            selected,
            list_ratings(&conn, &CountScope::default()).unwrap()
        );
        assert_eq!(selected, vec![(5, 3), (3, 1)]);
    }
}
