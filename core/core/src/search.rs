// SPDX-License-Identifier: MIT

use anyhow::Result;

/// Split user text into word tokens, dropping all punctuation/operators.
///
/// User input (especially pasted prose) routinely contains characters FTS5
/// treats as operators — `"` (phrase), `-` (NOT), `:` (column), `*` (prefix),
/// `(` `)` (grouping), and the `AND`/`OR`/`NOT` keywords. Reducing the input
/// to bare word tokens and re-quoting them ourselves means none of those can
/// reach the FTS5 parser, so a query can never raise a syntax error.
fn tokenize(raw: &str) -> Vec<String> {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Exact contiguous phrase, e.g. `"core rust concepts"*`. The whole input is
/// one quoted phrase (so FTS5 keywords/operators stay literal); the trailing
/// `*` makes only the final token a prefix, so a half-typed last word matches
/// as you type while the earlier, completed words stay exact.
fn phrase_query(tokens: &[String]) -> String {
    format!("\"{}\"*", tokens.join(" "))
}

/// All-words AND, e.g. `"core" "rust" "concepts"*`. Space-separated quoted
/// terms are implicitly AND-ed by FTS5: every word must appear in the article,
/// in any order or position. Only the final term gets a prefix `*` (the word
/// being typed); the rest must match in full.
fn and_query(tokens: &[String]) -> String {
    let last = tokens.len() - 1;
    tokens
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if i == last {
                format!("\"{t}\"*")
            } else {
                format!("\"{t}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build one FTS predicate that requires every selected visual term in the
/// derived `visual_terms` column. Each selected value is a completed token, so
/// multiword values stay contiguous phrases and do not use type-ahead prefix
/// matching.
pub(crate) fn scoped_visual_query(terms: &[String]) -> Option<String> {
    if terms.is_empty() {
        return Some(String::new());
    }
    let mut queries = Vec::with_capacity(terms.len());
    for term in terms {
        let tokens = tokenize(term);
        if tokens.is_empty() {
            return None;
        }
        queries.push(format!("visual_terms : \"{}\"", tokens.join(" ")));
    }
    Some(queries.join(" AND "))
}

/// Build the FTS5 `MATCH` string for `query`, preferring an exact phrase and
/// falling back to all-words-AND when the phrase matches nothing in the
/// caller's active scope. Returns `None` when there's nothing searchable
/// (blank / pure punctuation).
///
/// This is the single place user text becomes an FTS query; callers provide the
/// phrase-existence probe so fallback is resolved against the same filters as
/// the eventual result set rather than against the whole library.
///
/// `query` is plain user text. It is first matched as an exact contiguous
/// phrase; if that finds nothing, it falls back to requiring all the words to
/// appear anywhere in the article. The fallback matters because `body_text`
/// holds the raw Markdown, so link URLs and other markup are tokenized
/// *between* the visible words — pasted rendered prose rarely lines up as a
/// literal phrase, but every word is still present.
///
/// The FTS index spans the title, body, source site, *and tags*, so a query like
/// "nytimes" surfaces articles from nytimes.com even when the term appears
/// nowhere in their text — site tokens match (and prefix-match) just like any
/// other word. Tag values use the same plain-text matching behavior.
pub(crate) fn match_query(
    query: &str,
    phrase_exists: impl FnOnce(&str) -> Result<bool>,
) -> Result<Option<String>> {
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return Ok(None);
    }
    let phrase = phrase_query(&tokens);
    let phrase_matches = phrase_exists(&phrase)?;
    Ok(Some(if phrase_matches {
        phrase
    } else {
        and_query(&tokens)
    }))
}

/// Add an all-words fallback scoped only to derived visual terms. Text search
/// keeps its established phrase-first behaviour, while a phrase hit elsewhere
/// cannot hide a valid multiword image classification.
pub(crate) fn with_visual_fallback(raw: &str, resolved: &str) -> String {
    let tokens = tokenize(raw);
    if tokens.len() < 2 {
        return resolved.to_string();
    }
    format!("({resolved}) OR (visual_terms : ({}))", and_query(&tokens))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::{params, Connection};
    use tempfile::TempDir;

    use super::*;
    use crate::{index::open, new_id, reconcile::rebuild, write_reading, LibraryRoot, Metadata};

    fn make_library(dir: &TempDir) -> LibraryRoot {
        fs::create_dir_all(dir.path().join("articles")).unwrap();
        LibraryRoot::new(dir.path()).unwrap()
    }

    fn meta(id: &str, url: &str, title: &str) -> Metadata {
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
            title: title.to_string(),
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

    fn meta_site(id: &str, url: &str, title: &str, site: &str) -> Metadata {
        let mut m = meta(id, url, title);
        m.site = Some(site.to_string());
        m
    }

    fn setup() -> (TempDir, Connection) {
        let dir = TempDir::new().unwrap();
        let conn = open(&dir.path().join("index.db")).unwrap();
        (dir, conn)
    }

    /// Build the MATCH string for `query` via [`match_query`] and return the ids
    /// it selects, best-first. Exercises the real query builder against the FTS
    /// index — the same string `list_readings` runs — minus the list's
    /// view/tag/rating filters and pagination.
    fn matches(conn: &Connection, query: &str) -> Vec<String> {
        let Some(m) = match_query(query, |phrase| {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM readings_fts WHERE readings_fts MATCH ?1)",
                params![phrase],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
        .unwrap() else {
            return vec![];
        };
        let mut stmt = conn
            .prepare(
                "SELECT r.id
                 FROM readings_fts JOIN readings r ON r.rowid = readings_fts.rowid
                 WHERE readings_fts MATCH ?1
                 ORDER BY bm25(readings_fts)",
            )
            .unwrap();
        let ids = stmt
            .query_map(params![m], |row| row.get::<_, String>(0))
            .unwrap();
        ids.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn blank_query_yields_no_match_string() {
        let (_dir, conn) = setup();
        // A present-but-untokenizable query produces no MATCH at all — the
        // caller treats that as "search matching nothing", not a full listing.
        assert!(match_query("", |_| Ok(false)).unwrap().is_none());
        assert!(matches(&conn, "").is_empty());
    }

    #[test]
    fn finds_match_in_title() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com", "Rust Programming Language"),
            "Some body text.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        assert_eq!(matches(&conn, "rust"), vec![id]);
    }

    #[test]
    fn finds_match_in_body() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com", "An Article"),
            "Ownership and borrowing are core Rust concepts.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        assert_eq!(matches(&conn, "borrowing"), vec![id]);
    }

    #[test]
    fn no_match_returns_empty() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        write_reading(
            &lib,
            meta(&new_id(), "https://example.com", "Rust Article"),
            "Body about Rust.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        assert!(matches(&conn, "python").is_empty());
    }

    #[test]
    fn multi_word_phrase_matches_contiguous_body() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com", "An Article"),
            "Ownership and borrowing are core Rust concepts.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        assert_eq!(matches(&conn, "core Rust concepts"), vec![id]);
    }

    #[test]
    fn pasted_text_with_punctuation_matches_source() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com", "An Article"),
            "Ownership and borrowing are core Rust concepts.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // A pasted snippet carrying punctuation, a stray quote, a hyphen and a
        // colon must not raise an FTS5 syntax error and must still find the
        // source (both sides tokenize identically).
        assert_eq!(
            matches(&conn, "borrowing are \"core\" Rust-concepts:"),
            vec![id]
        );
    }

    #[test]
    fn non_contiguous_words_match_via_and_fallback() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com", "An Article"),
            "Ownership and borrowing are core Rust concepts.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // Not adjacent → no exact phrase match, but both words are present, so
        // the all-words AND fallback still finds the article.
        assert_eq!(matches(&conn, "Ownership concepts"), vec![id]);
    }

    #[test]
    fn pasted_prose_across_a_link_still_matches() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        // body_text is raw Markdown, so the link URL is tokenized *between*
        // the visible words: read the official docs <https doc rust ...> now.
        write_reading(
            &lib,
            meta(&id, "https://example.com", "Docs"),
            "Read the [official docs](https://doc.rust-lang.org/book) now.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // Pasting the rendered sentence can't match as a contiguous phrase
        // (the URL tokens interleave), but every word is present → AND fallback.
        assert_eq!(matches(&conn, "Read the official docs now"), vec![id]);
    }

    #[test]
    fn truncated_last_word_matches_via_phrase_prefix() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com", "Loops"),
            "Loop engineering is replacing yourself as the person who prompts the agent."
                .to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // Typing stops mid-word: "…prompts the ag". The trailing prefix lets
        // "ag" match "agent" so the result appears before the word is finished.
        assert_eq!(matches(&conn, "prompts the ag"), vec![id]);
    }

    #[test]
    fn truncated_last_word_matches_across_link_via_and_prefix() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com", "Docs"),
            "Read the [official docs](https://doc.rust-lang.org/book) now.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // Phrase fails (URL interleaves) AND the last word is truncated; the
        // AND fallback with a prefix on the final term still finds it.
        assert_eq!(matches(&conn, "official docs no"), vec![id]);
    }

    #[test]
    fn finds_match_by_site() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta_site(&id, "https://nytimes.com/a", "Some Headline", "nytimes.com"),
            "Body with no mention of the source.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // The term appears nowhere in title/body — only in the site column.
        assert_eq!(matches(&conn, "nytimes"), vec![id]);
    }

    #[test]
    fn finds_match_by_tag() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        let mut metadata = meta(&id, "https://example.com/tagged", "No matching words");
        metadata.tags = vec!["local-first".into(), "inspiration".into()];
        write_reading(&lib, metadata, "Body without the search term.".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        assert_eq!(matches(&conn, "inspiration"), vec![id.clone()]);
        assert_eq!(matches(&conn, "local"), vec![id]);
    }

    #[test]
    fn site_match_is_case_insensitive() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta_site(&id, "https://www.GitHub.com/x", "Repo", "www.GitHub.com"),
            "body".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // FTS lowercases tokens, so a lowercase query matches a mixed-case site.
        assert_eq!(matches(&conn, "github"), vec![id]);
    }

    #[test]
    fn site_prefix_match_works() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta_site(&id, "https://nytimes.com/a", "Headline", "nytimes.com"),
            "Body with no mention of the source.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // A half-typed site token prefix-matches, just like title/body terms:
        // the trailing-`*` on the last token lets "nyt" reach "nytimes".
        assert_eq!(matches(&conn, "nyt"), vec![id]);
    }

    #[test]
    fn site_and_content_matches_have_no_duplicates() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        // Article A: "rust" only in the body.
        let a = new_id();
        write_reading(
            &lib,
            meta_site(
                &a,
                "https://blog.example.com/a",
                "Post A",
                "blog.example.com",
            ),
            "All about rust and ownership.".to_string(),
        )
        .unwrap();

        // Article B: site is rust-lang.org; "rust" appears in the site only.
        let b = new_id();
        write_reading(
            &lib,
            meta_site(&b, "https://rust-lang.org/b", "Post B", "rust-lang.org"),
            "Body with no keyword.".to_string(),
        )
        .unwrap();

        // Article C: "rust" in BOTH body and site — a single MATCH must still
        // return it exactly once.
        let c = new_id();
        write_reading(
            &lib,
            meta_site(&c, "https://rust-lang.org/c", "Post C", "rust-lang.org"),
            "Learning rust today.".to_string(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();

        let ids = matches(&conn, "rust");
        assert!(ids.contains(&a), "body hit A should be present");
        assert!(ids.contains(&b), "site hit B should be present");
        assert!(ids.contains(&c), "hit C should be present");
        // C matches in two columns but FTS lists each row once.
        assert_eq!(ids.len(), 3, "C must not be duplicated");
    }

    #[test]
    fn operator_characters_are_treated_literally() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        write_reading(
            &lib,
            meta(&new_id(), "https://example.com", "Article"),
            "Asynchronous programming with async/await in Rust.".to_string(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        // A trailing `*` is no longer FTS5 prefix syntax; it's a literal,
        // ignorable separator, so this matches the `async` token without error.
        assert_eq!(matches(&conn, "async*").len(), 1);

        // Input that is nothing but operator characters tokenizes to an empty
        // phrase: zero matches, never an error.
        assert!(matches(&conn, "-\"()*:").is_empty());
    }
}
