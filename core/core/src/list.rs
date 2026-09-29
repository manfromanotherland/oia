// SPDX-License-Identifier: MIT

use anyhow::Result;
use rusqlite::{params, params_from_iter, types::Value, Connection};

use crate::{PredominantColor, ReadingKind, WeightedColor};

/// Smart-view filter applied when listing readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Every reading, including legacy archived rows.
    All,
    /// Non-archived readings that have not been read.
    Unread,
    /// Non-archived readings that have been read.
    Read,
    /// Archived readings.
    Archive,
    /// Readings marked as favorite (regardless of archived state).
    Favorites,
    /// Image and video readings.
    Media,
    /// Fully captured articles, excluding lightweight links and social posts.
    Articles,
    /// Readings with a personal `note.md` sidecar, regardless of card kind.
    Notes,
    /// Lightweight URL saves and captured social posts.
    Links,
    /// Selected-text and source-less text cards.
    Quotes,
}

/// Field to sort a listing by. Direction is controlled separately by
/// [`ListOptions::ascending`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    /// When the reading was saved (default).
    #[default]
    SavedAt,
    /// When the reading was last marked read. Unread rows (`read_at IS NULL`)
    /// always sort last, regardless of direction.
    ReadAt,
    /// Star rating (0–5).
    Rating,
    /// Estimated time to read, ranked by word count. Rows without a known
    /// word count (`word_count IS NULL`) always sort last, regardless of
    /// direction.
    WordCount,
    /// Full-text relevance (BM25), best first. Only meaningful with a query set;
    /// listings without one fall back to `SavedAt`.
    Relevance,
}

/// Options for `list_readings`.
#[derive(Debug, Clone)]
pub struct ListOptions {
    pub view: View,
    pub sort: SortField,
    /// Sort ascending when `true`, descending when `false`. Descending is the
    /// natural default for every field (newest / highest / most-recently-read
    /// first).
    pub ascending: bool,
    /// Restrict to readings that carry this tag (exact match).
    pub tag: Option<String>,
    /// Restrict to readings with this exact star rating, 1–5. `None` = no filter.
    pub rating: Option<u8>,
    /// Restrict to one persisted content kind. `None` includes every kind.
    pub kind: Option<ReadingKind>,
    /// ISO-8601 lower bound on `saved_at` (inclusive).
    pub since: Option<String>,
    /// ISO-8601 upper bound on `saved_at` (inclusive).
    pub until: Option<String>,
    /// Full-text query, or `#RRGGBB` / `colour:#RRGGBB` for perceptually similar palette
    /// clusters. `None` is a plain listing. Relevance ranks shade matches by
    /// distance; platform semantic candidates cannot widen a colour query.
    pub query: Option<String>,
    /// Exact tags that must all occur on the same reading. Unlike `tag`, these
    /// are search terms rather than a single board facet selection.
    pub tag_terms: Vec<String>,
    /// Completed terms that must all occur in the reading's current supported
    /// visual analysis. They never match title, body, site, or tags.
    pub visual_terms: Vec<String>,
    /// Palette colors selected as search tokens, combined by AND.
    pub color_terms: Vec<String>,
    /// Item types selected as search terms; every term must match.
    pub item_type_terms: Vec<String>,
    /// Restrict to the stable colour family derived by the Rust core.
    pub predominant_color: Option<PredominantColor>,
    /// Ordered Core Spotlight candidates for the same query.
    pub semantic_candidate_ids: Vec<String>,
    /// Core Spotlight candidates produced from the structured visual terms.
    /// These are kept separate from free-text candidates so metadata matches
    /// can never widen an "In this image" token.
    pub visual_semantic_candidate_ids: Vec<String>,
    pub limit: usize,
    pub offset: usize,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            view: View::All,
            sort: SortField::SavedAt,
            ascending: false,
            tag: None,
            rating: None,
            kind: None,
            since: None,
            until: None,
            query: None,
            tag_terms: Vec::new(),
            visual_terms: Vec::new(),
            color_terms: Vec::new(),
            item_type_terms: Vec::new(),
            predominant_color: None,
            semantic_candidate_ids: Vec::new(),
            visual_semantic_candidate_ids: Vec::new(),
            limit: 50,
            offset: 0,
        }
    }
}

/// Lightweight row returned by `list_readings` — no body text.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadingRow {
    pub id: String,
    pub title: String,
    pub kind: ReadingKind,
    pub lightweight: bool,
    /// Board classification. Captured social posts remain full local readings
    /// even though the board groups them with links.
    pub is_link: bool,
    pub has_note: bool,
    pub url: String,
    pub media_url: Option<String>,
    pub preview_asset: Option<String>,
    pub favicon_asset: Option<String>,
    pub theme_color: Option<String>,
    /// Opaque disposable projection of the typed frontmatter source profile.
    pub source_profile_json: Option<String>,
    /// Representative exact sRGB cluster from the current derived visual
    /// analysis. A large extreme-neutral canvas may yield to a substantial
    /// secondary surface colour. This is presentation metadata only; the
    /// palette cache remains disposable and the reading file stays authoritative.
    pub dominant_color: Option<WeightedColor>,
    pub media_aspect_ratio: Option<f64>,
    pub canonical_url: String,
    pub author: Option<String>,
    pub site: Option<String>,
    pub saved_at: String,
    /// `true` when the reading has been read — derived from `read_at` being set.
    pub read: bool,
    /// UTC ISO-8601 timestamp of the most recent time marked read, or `None`.
    pub read_at: Option<String>,
    pub archived: bool,
    pub favorite: bool,
    pub rating: u8,
    pub excerpt: Option<String>,
    /// Bounded board-only description derived from the saved excerpt or body.
    /// The file's excerpt is preserved separately above.
    pub card_description: Option<String>,
    pub word_count: Option<u32>,
    pub lang: Option<String>,
    pub tags: Vec<String>,
}

/// The SQL predicate that selects a smart view. Single source of truth shared
/// by [`list_readings`] and [`view_counts`] so a view's count can never
/// disagree with the list it produces.
pub(crate) fn view_clause(view: View) -> &'static str {
    match view {
        View::All => "1 = 1",
        View::Unread => "archived = 0 AND read_at IS NULL",
        View::Read => "archived = 0 AND read_at IS NOT NULL",
        View::Archive => "archived = 1",
        View::Favorites => "favorite = 1",
        View::Media => "kind IN ('image', 'video')",
        View::Articles => "kind = 'article' AND lightweight = 0 AND json_extract(source_profile_json, '$.source_type') IS NOT 'social_post'",
        View::Notes => "has_note = 1",
        View::Links => "kind = 'article' AND (lightweight = 1 OR json_extract(source_profile_json, '$.source_type') IS 'social_post')",
        View::Quotes => "kind = 'quote'",
    }
}

/// The number of readings in each smart view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewCounts {
    pub all: u64,
    pub unread: u64,
    pub read: u64,
    pub archive: u64,
    pub favorites: u64,
}

/// Every sidebar count section — the view badges, the tag counts, and the rating
/// counts — gathered in a single call. See [`sidebar_counts`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SidebarCounts {
    /// Per-smart-view badge counts.
    pub views: ViewCounts,
    /// `(tag, count)` for every tag in the library, alphabetical (`list_tags`).
    pub tags: Vec<(String, u64)>,
    /// `(rating, count)` for every in-use star bucket, highest first
    /// (`list_ratings`).
    pub ratings: Vec<(u8, u64)>,
}

/// The active board filters that scope the faceted counts. The selected smart
/// view, tag facet, rating, kind, and every free-text or structured search term
/// compose as an intersection. A facet may be unset and the structured search
/// vectors may be empty (`view` defaults to `All`, the unfiltered base).
///
/// Each count query applies every field of the scope *except its own axis* — a
/// facet never constrains itself, so its badges still show the alternatives you
/// could switch to (see [`count_where`]). This is standard faceted navigation:
/// selecting 5★ recounts the Library and Tags sections against the 5★ subset
/// while the Ratings section keeps showing every rating you could pick instead.
#[derive(Debug, Clone)]
pub struct CountScope {
    /// The selected smart view (`All` when none is chosen — the unfiltered base).
    /// Constrains the tag and rating counts; ignored by [`view_counts`] itself
    /// (which groups every view via `FILTER`).
    pub view: View,
    /// The selected tag, if one is active. Constrains the view and rating counts;
    /// ignored by [`list_tags`].
    pub tag: Option<String>,
    /// The selected rating, if one is active. Constrains the view and tag counts;
    /// ignored by [`list_ratings`].
    pub rating: Option<u8>,
    /// The selected content kind. It composes with every sidebar count section.
    pub kind: Option<ReadingKind>,
    /// The active full-text query. Composes with every facet. `None` means no
    /// search; a present-but-unmatchable query scopes every count to zero,
    /// mirroring [`list_readings`].
    pub query: Option<String>,
    /// Exact tag search terms. Every value must be present on the same reading.
    pub tag_terms: Vec<String>,
    /// Visual search terms. Every value must occur in the same current,
    /// supported visual analysis.
    pub visual_terms: Vec<String>,
    pub predominant_color: Option<PredominantColor>,
    pub semantic_candidate_ids: Vec<String>,
    pub visual_semantic_candidate_ids: Vec<String>,
}

impl Default for CountScope {
    fn default() -> Self {
        Self {
            view: View::All,
            tag: None,
            rating: None,
            kind: None,
            query: None,
            tag_terms: Vec::new(),
            visual_terms: Vec::new(),
            predominant_color: None,
            semantic_candidate_ids: Vec::new(),
            visual_semantic_candidate_ids: Vec::new(),
        }
    }
}

/// The facet axis a count query groups by — the one scope filter it must skip so
/// a section never constrains its own badges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Facet {
    View,
    Tag,
    Rating,
}

/// A [`CountScope`]'s search resolved to its FTS state exactly once. Resolving
/// the `MATCH` string probes the FTS index (the phrase-vs-AND fallback in
/// [`crate::search::match_query`]), so [`sidebar_counts`] plans it a single time
/// and shares the result across all three sections.
#[derive(Debug, Clone)]
pub(crate) enum ResolvedSearch {
    /// The scope carried no query — no full-text predicate applies.
    Unfiltered,
    /// A query was present but tokenized/matched to nothing; every count is zero.
    Unmatchable,
    /// Ordered semantic candidates exist, but the text tokenized to nothing.
    Semantic(String),
    /// The query resolved to FTS5, optionally unioned with semantic candidates.
    Fts { query: String, semantic: String },
}

impl ResolvedSearch {
    /// Resolve a search against the complete active count scope. The selected
    /// view and sibling facets determine phrase fallback, while the later count
    /// queries may still ignore their own facet axis when presenting choices.
    pub(crate) fn resolve_scoped(conn: &Connection, scope: &CountScope) -> Result<Self> {
        if crate::search::scoped_visual_query(&scope.visual_terms).is_none() {
            return Ok(Self::Unmatchable);
        }
        if let Some(color) = scope.query.as_deref().and_then(crate::color_search::parse) {
            return Ok(Self::Semantic(serde_json::to_string(
                &crate::color_search::matching_ids(conn, &color)?,
            )?));
        }
        Ok(match scope.query.as_deref() {
            None => Self::Unfiltered,
            Some(q) => match crate::search::match_query(q, |phrase| {
                phrase_exists_in_count_scope(conn, scope, phrase)
            })? {
                None if scope.semantic_candidate_ids.is_empty() => Self::Unmatchable,
                None => Self::Semantic(serde_json::to_string(&scope.semantic_candidate_ids)?),
                Some(query) => Self::Fts {
                    query: crate::search::with_visual_fallback(q, &query),
                    semantic: serde_json::to_string(&scope.semantic_candidate_ids)?,
                },
            },
        })
    }
}

/// Whether an exact phrase matches anywhere in the active count scope. This
/// probe intentionally has no pagination: phrase-vs-AND semantics belong to the
/// filtered result set as a whole, not whichever page happens to be requested.
fn phrase_exists_in_count_scope(
    conn: &Connection,
    scope: &CountScope,
    phrase: &str,
) -> Result<bool> {
    let view_clause = view_clause(scope.view);
    let sql = format!(
        "SELECT EXISTS(
             SELECT 1
             FROM readings_fts
             JOIN readings r ON r.rowid = readings_fts.rowid
             WHERE readings_fts MATCH ?1
               AND {view_clause}
               AND (?2 IS NULL OR EXISTS (
                    SELECT 1 FROM json_each(r.tags_json) WHERE value = ?2
               ))
               AND (?3 IS NULL OR r.rating = ?3)
               AND (?4 IS NULL OR r.kind = ?4)
               AND (?5 IS NULL OR r.predominant_color = ?5)
               AND (?6 = '' OR (
                    (
                        EXISTS (
                            SELECT 1 FROM visual_analysis a
                            WHERE a.content_hash=r.visual_asset_hash
                              AND a.analyzer_version=r.visual_analyzer_version
                              AND a.supported=1
                              AND a.visual_terms=r.visual_terms
                        )
                        AND r.rowid IN (
                            SELECT rowid FROM readings_fts WHERE readings_fts MATCH ?6
                        )
                    )
                    OR r.id IN (SELECT value FROM json_each(?8))
               ))
               AND NOT EXISTS (
                    SELECT 1 FROM json_each(?7) requested_tag
                    WHERE NOT EXISTS (
                        SELECT 1 FROM json_each(r.tags_json) reading_tag
                        WHERE reading_tag.value = requested_tag.value
                    )
               )
         )"
    );
    conn.query_row(
        &sql,
        params![
            phrase,
            scope.tag.as_deref(),
            scope.rating.map(i64::from),
            scope.kind.map(ReadingKind::as_str),
            scope.predominant_color.map(PredominantColor::as_str),
            crate::search::scoped_visual_query(&scope.visual_terms).unwrap_or_default(),
            serde_json::to_string(&scope.tag_terms)?,
            serde_json::to_string(&scope.visual_semantic_candidate_ids)?
        ],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

/// Append the structured search predicates shared by every count surface.
/// They are search terms rather than facets, so no count axis may skip them.
fn append_structured_count_clauses(
    scope: &CountScope,
    clauses: &mut Vec<String>,
    vals: &mut Vec<Value>,
) -> bool {
    if !scope.tag_terms.is_empty() {
        let tags_json = serde_json::to_string(&scope.tag_terms)
            .expect("serializing a string vector cannot fail");
        vals.push(Value::Text(tags_json));
        clauses.push(format!(
            "NOT EXISTS (
                 SELECT 1 FROM json_each(?{}) requested_tag
                 WHERE NOT EXISTS (
                     SELECT 1 FROM json_each(readings.tags_json) reading_tag
                     WHERE reading_tag.value = requested_tag.value
                 )
             )",
            vals.len()
        ));
    }

    let Some(visual_query) = crate::search::scoped_visual_query(&scope.visual_terms) else {
        return false;
    };
    if !visual_query.is_empty() {
        vals.push(Value::Text(visual_query));
        let visual_query_index = vals.len();
        vals.push(Value::Text(
            serde_json::to_string(&scope.visual_semantic_candidate_ids)
                .expect("serializing a string vector cannot fail"),
        ));
        let visual_semantic_index = vals.len();
        clauses.push(format!(
            "(
                 (
                     EXISTS (
                         SELECT 1 FROM visual_analysis a
                         WHERE a.content_hash=readings.visual_asset_hash
                           AND a.analyzer_version=readings.visual_analyzer_version
                           AND a.supported=1
                           AND a.visual_terms=readings.visual_terms
                     )
                     AND readings.rowid IN (
                         SELECT rowid FROM readings_fts WHERE readings_fts MATCH ?{visual_query_index}
                     )
                 )
                 OR readings.id IN (
                     SELECT value FROM json_each(?{visual_semantic_index})
                 )
             )"
        ));
    }
    true
}

/// Build the shared `WHERE` fragment (and its positional bind values) for a
/// faceted count over `readings`, applying every [`CountScope`] filter except
/// the one for `axis`, plus the pre-resolved `search`. Columns are qualified
/// `readings.` so the fragment is safe to splice into `list_tags`'
/// `readings, json_each(...)` cross join.
///
/// Returns `None` when `search` is [`ResolvedSearch::Unmatchable`]: the caller
/// yields empty counts rather than a full listing, exactly as `list_readings`
/// treats an unmatchable search as "no results".
pub(crate) fn count_where(
    scope: &CountScope,
    axis: Facet,
    search: &ResolvedSearch,
) -> Option<(String, Vec<Value>)> {
    let mut clauses: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();

    // The selected view constrains tag/rating counts; view_counts skips it and
    // partitions every view itself via FILTER.
    if axis != Facet::View {
        clauses.push(view_clause(scope.view).to_string());
    }

    if axis != Facet::Tag {
        if let Some(tag) = scope.tag.as_deref() {
            vals.push(Value::Text(tag.to_string()));
            clauses.push(format!(
                "EXISTS (SELECT 1 FROM json_each(readings.tags_json) WHERE value = ?{})",
                vals.len()
            ));
        }
    }

    if axis != Facet::Rating {
        if let Some(rating) = scope.rating {
            vals.push(Value::Integer(rating as i64));
            clauses.push(format!("readings.rating = ?{}", vals.len()));
        }
    }

    if let Some(kind) = scope.kind {
        vals.push(Value::Text(kind.as_str().to_string()));
        clauses.push(format!("readings.kind = ?{}", vals.len()));
    }

    if let Some(color) = scope.predominant_color {
        vals.push(Value::Text(color.as_str().to_string()));
        clauses.push(format!("readings.predominant_color = ?{}", vals.len()));
    }

    if !append_structured_count_clauses(scope, &mut clauses, &mut vals) {
        return None;
    }

    // A search composes with every facet; the caller resolved it once so counts
    // and results always agree. An unmatchable search means the whole count is
    // empty.
    match search {
        ResolvedSearch::Unmatchable => return None,
        ResolvedSearch::Fts { query, semantic } => {
            vals.push(Value::Text(query.clone()));
            let fts_param = vals.len();
            vals.push(Value::Text(semantic.clone()));
            clauses.push(format!(
                "(readings.rowid IN \
                 (SELECT rowid FROM readings_fts WHERE readings_fts MATCH ?{fts_param}) \
                 OR readings.id IN (SELECT value FROM json_each(?{})))",
                vals.len(),
            ));
        }
        ResolvedSearch::Semantic(semantic) => {
            vals.push(Value::Text(semantic.clone()));
            clauses.push(format!(
                "readings.id IN (SELECT value FROM json_each(?{}))",
                vals.len()
            ));
        }
        ResolvedSearch::Unfiltered => {}
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    Some((where_sql, vals))
}

/// Build the `COUNT(...) FILTER (WHERE …)` predicate for a *pinned* facet section
/// (Tags or Ratings). The presence set (which tiles show) is every tag / rating
/// in the library, regardless of view, so the badge must re-apply the *full*
/// faceted scope: the exact view clause, the pre-resolved `search`, and the
/// sibling facet. `sibling` is the cross facet that is neither the section's own
/// axis nor the view (`Facet::Rating` for the Tags section, `Facet::Tag` for the
/// Ratings section).
///
/// Returns the predicate SQL and pushes any bound values onto `vals`. An
/// [`ResolvedSearch::Unmatchable`] search contributes the constant-false
/// predicate `0`, so every tile reports 0 while staying pinned, rather than the
/// section emptying out.
pub(crate) fn pinned_count_filter(
    scope: &CountScope,
    sibling: Facet,
    search: &ResolvedSearch,
    vals: &mut Vec<Value>,
) -> String {
    // Start from the exact view clause: presence is broader than the view (e.g.
    // Unread pins every non-archived tag), so the badge itself must narrow back to
    // the actual view.
    let mut conds: Vec<String> = vec![view_clause(scope.view).to_string()];

    match search {
        ResolvedSearch::Unmatchable => conds.push("0".to_string()),
        ResolvedSearch::Fts { query, semantic } => {
            vals.push(Value::Text(query.clone()));
            let fts_param = vals.len();
            vals.push(Value::Text(semantic.clone()));
            conds.push(format!(
                "(readings.rowid IN \
                 (SELECT rowid FROM readings_fts WHERE readings_fts MATCH ?{fts_param}) \
                 OR readings.id IN (SELECT value FROM json_each(?{})))",
                vals.len(),
            ));
        }
        ResolvedSearch::Semantic(semantic) => {
            vals.push(Value::Text(semantic.clone()));
            conds.push(format!(
                "readings.id IN (SELECT value FROM json_each(?{}))",
                vals.len()
            ));
        }
        ResolvedSearch::Unfiltered => {}
    }

    match sibling {
        Facet::Rating => {
            if let Some(rating) = scope.rating {
                vals.push(Value::Integer(rating as i64));
                conds.push(format!("readings.rating = ?{}", vals.len()));
            }
        }
        Facet::Tag => {
            if let Some(tag) = scope.tag.as_deref() {
                vals.push(Value::Text(tag.to_string()));
                conds.push(format!(
                    "EXISTS (SELECT 1 FROM json_each(readings.tags_json) WHERE value = ?{})",
                    vals.len()
                ));
            }
        }
        Facet::View => unreachable!("view is the presence axis, never a sibling"),
    }

    if let Some(kind) = scope.kind {
        vals.push(Value::Text(kind.as_str().to_string()));
        conds.push(format!("readings.kind = ?{}", vals.len()));
    }

    if let Some(color) = scope.predominant_color {
        vals.push(Value::Text(color.as_str().to_string()));
        conds.push(format!("readings.predominant_color = ?{}", vals.len()));
    }

    if !append_structured_count_clauses(scope, &mut conds, vals) {
        conds.push("0".to_string());
    }

    conds.join(" AND ")
}

/// Count the readings in every smart view in a single pass over the table,
/// scoped by the active search and cross-facet selection (tag/rating) so the
/// Library badges track what a search or facet actually narrows to.
///
/// One grouped aggregate — not five `SELECT COUNT(*)`s, and emphatically not
/// `list_readings(..).len()` — so the cost is a single table scan regardless of
/// library size, with no rows materialized and no `LIMIT` to silently cap the
/// result. The `WHERE` restricts the population to the scope (minus the view
/// axis); each `FILTER` then partitions that population per view. `FILTER` needs
/// SQLite >= 3.30 (satisfied by the bundled build) and yields 0 rather than NULL
/// for an empty view.
pub fn view_counts(conn: &Connection, scope: &CountScope) -> Result<ViewCounts> {
    let search = ResolvedSearch::resolve_scoped(conn, scope)?;
    view_counts_with(conn, scope, &search)
}

/// [`view_counts`] with the search pre-resolved, so [`sidebar_counts`] can share
/// one resolution across all three sections.
pub(crate) fn view_counts_with(
    conn: &Connection,
    scope: &CountScope,
    search: &ResolvedSearch,
) -> Result<ViewCounts> {
    let Some((where_sql, vals)) = count_where(scope, Facet::View, search) else {
        // A present-but-unmatchable search scopes every view to zero.
        return Ok(ViewCounts::default());
    };
    let sql = format!(
        "SELECT
             COUNT(*),
             COUNT(*) FILTER (WHERE archived = 0 AND read_at IS NULL),
             COUNT(*) FILTER (WHERE archived = 0 AND read_at IS NOT NULL),
             COUNT(*) FILTER (WHERE archived = 1),
             COUNT(*) FILTER (WHERE favorite = 1)
         FROM readings {where_sql}"
    );
    conn.query_row(&sql, params_from_iter(vals), |row| {
        Ok(ViewCounts {
            all: row.get::<_, i64>(0)? as u64,
            unread: row.get::<_, i64>(1)? as u64,
            read: row.get::<_, i64>(2)? as u64,
            archive: row.get::<_, i64>(3)? as u64,
            favorites: row.get::<_, i64>(4)? as u64,
        })
    })
    .map_err(Into::into)
}

/// Compute all three sidebar sections — the view badges, the tag counts, and the
/// rating counts — in one call, resolving the search's FTS `MATCH` string a
/// single time and threading it through every section. The sidebar always
/// recounts all three together on a settled search or facet change, so gathering
/// them here plans the query once and returns them in a single pass.
pub fn sidebar_counts(conn: &Connection, scope: &CountScope) -> Result<SidebarCounts> {
    let search = ResolvedSearch::resolve_scoped(conn, scope)?;
    Ok(SidebarCounts {
        views: view_counts_with(conn, scope, &search)?,
        tags: crate::tags::list_tags_with(conn, scope, &search)?,
        ratings: crate::rating::list_ratings_with(conn, scope, &search)?,
    })
}

/// List readings from the index according to `opts`.
///
/// When `opts.query` is set, rows are filtered through the full-text index
/// (composing with the view/tag/rating/kind/date filters) and rank by BM25 under
/// `SortField::Relevance`. Otherwise this is a plain listing over the `readings`
/// table.
pub fn list_readings(conn: &Connection, opts: &ListOptions) -> Result<Vec<ReadingRow>> {
    let Some(visual_query) = crate::search::scoped_visual_query(&opts.visual_terms) else {
        return Ok(Vec::new());
    };
    let color_ids = crate::color_search::matching_ids_for_terms(conn, &opts.color_terms)?;
    if color_ids.as_ref().is_some_and(Vec::is_empty) {
        return Ok(Vec::new());
    }
    let color_ids_json = color_ids
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?
        .unwrap_or_default();
    if let Some(color) = opts.query.as_deref().and_then(crate::color_search::parse) {
        let mut color_options = opts.clone();
        color_options.semantic_candidate_ids = crate::color_search::matching_ids(conn, &color)?;
        return list_readings_search(conn, &color_options, None, &color_ids_json);
    }
    // A present query means "search" — even whitespace/punctuation-only input,
    // which matches nothing rather than falling back to the full listing.
    if let Some(query) = opts.query.as_deref() {
        return match crate::search::match_query(query, |phrase| {
            phrase_exists_in_list_scope(conn, opts, phrase, &color_ids_json)
        })? {
            Some(match_query) => {
                let match_query = crate::search::with_visual_fallback(query, &match_query);
                list_readings_search(conn, opts, Some(&match_query), &color_ids_json)
            }
            None if opts.semantic_candidate_ids.is_empty() => Ok(Vec::new()),
            None => list_readings_search(conn, opts, None, &color_ids_json),
        };
    }

    if let Some(color_ids) = color_ids {
        let mut color_options = opts.clone();
        color_options.semantic_candidate_ids = color_ids;
        return list_readings_search(conn, &color_options, None, &color_ids_json);
    }

    let view_clause = view_clause(opts.view);

    // Direction applies to the chosen field; a final `id DESC` makes the order
    // total so pagination is stable when the primary key ties. For `read_at`,
    // the leading `(read_at IS NULL)` term forces unread rows last in both
    // directions (it always sorts ascending: 0 = has-date before 1 = null).
    let dir = if opts.ascending { "ASC" } else { "DESC" };
    let order = match opts.sort {
        // Relevance needs the FTS table; off-search it's meaningless, so fall
        // back to the default field.
        SortField::SavedAt | SortField::Relevance => format!("saved_at {dir}, id DESC"),
        SortField::ReadAt => format!("(read_at IS NULL), read_at {dir}, id DESC"),
        SortField::Rating => format!("rating {dir}, id DESC"),
        SortField::WordCount => format!("(word_count IS NULL), word_count {dir}, id DESC"),
    };

    // Optional filters use sentinel values (empty string / 0) so the SQL is
    // always static with exactly 11 bound parameters — no dynamic param count.
    let sql = format!(
        "SELECT id, title, url, canonical_url, author, site, saved_at,
                (read_at IS NOT NULL), archived, favorite, excerpt, word_count, lang, tags_json,
                rating, read_at, kind, media_url, preview_asset, favicon_asset, theme_color,
                lightweight, has_note, media_aspect_ratio,
                (SELECT CASE WHEN json_valid(a.palette_json) THEN a.palette_json ELSE '[]' END
                 FROM visual_analysis a
                 WHERE a.content_hash=readings.visual_asset_hash
                   AND a.analyzer_version=readings.visual_analyzer_version
                   AND a.supported=1),
                source_profile_json, card_description
         FROM readings
         WHERE {view_clause}
           AND (?3 = '' OR EXISTS (SELECT 1 FROM json_each(tags_json) WHERE value = ?3))
           AND (?4 = '' OR saved_at >= ?4)
           AND (?5 = '' OR saved_at <= ?5)
           AND (?6 = 0 OR rating = ?6)
           AND (?7 = '' OR kind = ?7)
           AND (?8 = '' OR predominant_color = ?8)
           AND (?9 = '' OR (
                (
                    EXISTS (
                        SELECT 1 FROM visual_analysis a
                        WHERE a.content_hash=readings.visual_asset_hash
                          AND a.analyzer_version=readings.visual_analyzer_version
                          AND a.supported=1
                          AND a.visual_terms=readings.visual_terms
                    )
                    AND readings.rowid IN (
                        SELECT rowid FROM readings_fts WHERE readings_fts MATCH ?9
                    )
                )
                OR readings.id IN (SELECT value FROM json_each(?11))
           ))
           AND NOT EXISTS (
                SELECT 1 FROM json_each(?10) requested_tag
                WHERE NOT EXISTS (
                    SELECT 1 FROM json_each(readings.tags_json) reading_tag
                    WHERE reading_tag.value = requested_tag.value
                )
           )
           AND (?12 = '' OR readings.id IN (SELECT value FROM json_each(?12)))
           AND NOT EXISTS (
               SELECT 1 FROM json_each(?13) requested_type
               WHERE NOT (
                   (readings.kind = requested_type.value AND requested_type.value IN ('image', 'video', 'quote'))
                   OR (readings.kind = 'article' AND requested_type.value = 'article'
                       AND readings.lightweight = 0
                       AND json_extract(readings.source_profile_json, '$.source_type') IS NOT 'social_post')
                   OR (readings.kind = 'article' AND requested_type.value = 'link'
                       AND (readings.lightweight = 1
                            OR json_extract(readings.source_profile_json, '$.source_type') IS 'social_post'))
               )
           )
         ORDER BY {order}
         LIMIT ?1 OFFSET ?2"
    );

    let mut stmt = conn.prepare(&sql)?;

    let tag_val = opts.tag.as_deref().unwrap_or("");
    let since_val = opts.since.as_deref().unwrap_or("");
    let until_val = opts.until.as_deref().unwrap_or("");
    let rating_val = opts.rating.unwrap_or(0) as i64;
    let kind_val = opts.kind.map(ReadingKind::as_str).unwrap_or("");
    let color_val = opts
        .predominant_color
        .map(PredominantColor::as_str)
        .unwrap_or("");

    let tag_terms_json = serde_json::to_string(&opts.tag_terms)?;
    let rows = stmt.query_map(
        params![
            opts.limit as i64,
            opts.offset as i64,
            tag_val,
            since_val,
            until_val,
            rating_val,
            kind_val,
            color_val,
            visual_query,
            tag_terms_json,
            serde_json::to_string(&opts.visual_semantic_candidate_ids)?,
            color_ids_json,
            serde_json::to_string(&opts.item_type_terms)?,
        ],
        parse_row,
    )?;

    rows.map(|r| r.map_err(Into::into)).collect()
}

/// Whether an exact phrase matches anywhere in the fully filtered list scope.
/// `limit` and `offset` are deliberately absent so moving between pages can
/// never change the chosen phrase-vs-AND search semantics.
fn phrase_exists_in_list_scope(
    conn: &Connection,
    opts: &ListOptions,
    phrase: &str,
    color_ids_json: &str,
) -> Result<bool> {
    let view_clause = view_clause(opts.view);
    let sql = format!(
        "SELECT EXISTS(
             SELECT 1
             FROM readings_fts
             JOIN readings r ON r.rowid = readings_fts.rowid
             WHERE readings_fts MATCH ?1
               AND {view_clause}
               AND (?2 = '' OR EXISTS (
                    SELECT 1 FROM json_each(r.tags_json) WHERE value = ?2
               ))
               AND (?3 = '' OR r.saved_at >= ?3)
               AND (?4 = '' OR r.saved_at <= ?4)
               AND (?5 = 0 OR r.rating = ?5)
               AND (?6 = '' OR r.kind = ?6)
               AND (?7 = '' OR r.predominant_color = ?7)
               AND (?8 = '' OR (
                    (
                        EXISTS (
                            SELECT 1 FROM visual_analysis a
                            WHERE a.content_hash=r.visual_asset_hash
                              AND a.analyzer_version=r.visual_analyzer_version
                              AND a.supported=1
                              AND a.visual_terms=r.visual_terms
                        )
                        AND r.rowid IN (
                            SELECT rowid FROM readings_fts WHERE readings_fts MATCH ?8
                        )
                    )
                    OR r.id IN (SELECT value FROM json_each(?10))
               ))
               AND NOT EXISTS (
                    SELECT 1 FROM json_each(?9) requested_tag
                    WHERE NOT EXISTS (
                        SELECT 1 FROM json_each(r.tags_json) reading_tag
                        WHERE reading_tag.value = requested_tag.value
                    )
               )
               AND (?11 = '' OR r.id IN (SELECT value FROM json_each(?11)))
           AND NOT EXISTS (
               SELECT 1 FROM json_each(?12) requested_type
               WHERE NOT (
                   (r.kind = requested_type.value AND requested_type.value IN ('image', 'video', 'quote'))
                   OR (r.kind = 'article' AND requested_type.value = 'article'
                       AND r.lightweight = 0
                       AND json_extract(r.source_profile_json, '$.source_type') IS NOT 'social_post')
                   OR (r.kind = 'article' AND requested_type.value = 'link'
                       AND (r.lightweight = 1
                            OR json_extract(r.source_profile_json, '$.source_type') IS 'social_post'))
               )
           )
         )"
    );
    conn.query_row(
        &sql,
        params![
            phrase,
            opts.tag.as_deref().unwrap_or(""),
            opts.since.as_deref().unwrap_or(""),
            opts.until.as_deref().unwrap_or(""),
            opts.rating.unwrap_or(0) as i64,
            opts.kind.map(ReadingKind::as_str).unwrap_or(""),
            opts.predominant_color
                .map(PredominantColor::as_str)
                .unwrap_or(""),
            crate::search::scoped_visual_query(&opts.visual_terms).unwrap_or_default(),
            serde_json::to_string(&opts.tag_terms)?,
            serde_json::to_string(&opts.visual_semantic_candidate_ids)?,
            color_ids_json,
            serde_json::to_string(&opts.item_type_terms)?,
        ],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

/// Fetch the full content (metadata + body) of a single reading by id.
///
/// Returns `None` if the id is not in the index.
pub fn get_reading(conn: &Connection, id: &str) -> Result<Option<(ReadingRow, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, url, canonical_url, author, site, saved_at,
                (read_at IS NOT NULL), archived, favorite, excerpt, word_count, lang, tags_json,
                rating, read_at, kind, media_url, preview_asset, favicon_asset, theme_color,
                lightweight, has_note, media_aspect_ratio,
                (SELECT CASE WHEN json_valid(a.palette_json) THEN a.palette_json ELSE '[]' END
                 FROM visual_analysis a
                 WHERE a.content_hash=readings.visual_asset_hash
                   AND a.analyzer_version=readings.visual_analyzer_version
                   AND a.supported=1),
                source_profile_json, card_description,
                body_text
         FROM readings WHERE id = ?1",
    )?;

    let mut rows = stmt.query_map(params![id], |row| {
        let row_data = parse_row(row)?;
        let body: String = row.get(27)?;
        Ok((row_data, body))
    })?;

    match rows.next() {
        None => Ok(None),
        Some(r) => Ok(Some(r?)),
    }
}

/// Full-text variant of [`list_readings`]: filter rows through the FTS index
/// with `match_query` and rank by BM25 (`SortField::Relevance`) or the requested
/// field — all while honouring the view/tag/rating/kind/date filters and pagination.
fn list_readings_search(
    conn: &Connection,
    opts: &ListOptions,
    match_query: Option<&str>,
    color_ids_json: &str,
) -> Result<Vec<ReadingRow>> {
    let view_clause = view_clause(opts.view);
    let dir = if opts.ascending { "ASC" } else { "DESC" };
    // bm25() scores better matches lower, so plain ascending order is best-first.
    let order = match opts.sort {
        SortField::Relevance => {
            "m.source_rank ASC, m.text_score ASC, m.semantic_rank ASC, r.id DESC".to_string()
        }
        SortField::SavedAt => format!("r.saved_at {dir}, r.id DESC"),
        SortField::ReadAt => format!("(r.read_at IS NULL), r.read_at {dir}, r.id DESC"),
        SortField::Rating => format!("r.rating {dir}, r.id DESC"),
        SortField::WordCount => format!("(r.word_count IS NULL), r.word_count {dir}, r.id DESC"),
    };

    let semantic_json = serde_json::to_string(&opts.semantic_candidate_ids)?;
    let matched = if match_query.is_some() {
        "matched(rowid, source_rank, text_score, semantic_rank) AS (
             SELECT readings_fts.rowid, 0,
                    bm25(readings_fts, 8.0, 4.0, 2.0, 3.0, 0.7), NULL
             FROM readings_fts WHERE readings_fts MATCH ?9
             UNION ALL
             SELECT r.rowid, 1, 0.0, s.semantic_rank
             FROM semantic s JOIN readings r ON r.id=s.id
             WHERE r.rowid NOT IN (
                 SELECT rowid FROM readings_fts WHERE readings_fts MATCH ?9
             )
         )"
    } else {
        "matched(rowid, source_rank, text_score, semantic_rank) AS (
             SELECT r.rowid, 1, 0.0, s.semantic_rank
             FROM semantic s JOIN readings r ON r.id=s.id
         )"
    };
    let sql = format!(
        "WITH semantic(id, semantic_rank) AS (
             SELECT value, MIN(CAST(key AS INTEGER)) FROM json_each(?10) GROUP BY value
         ),
         {matched}
         SELECT r.id, r.title, r.url, r.canonical_url, r.author, r.site, r.saved_at,
                (r.read_at IS NOT NULL), r.archived, r.favorite, r.excerpt, r.word_count,
                r.lang, r.tags_json, r.rating, r.read_at, r.kind, r.media_url, r.preview_asset,
                r.favicon_asset, r.theme_color, r.lightweight, r.has_note,
                r.media_aspect_ratio,
                (SELECT CASE WHEN json_valid(a.palette_json) THEN a.palette_json ELSE '[]' END
                 FROM visual_analysis a
                 WHERE a.content_hash=r.visual_asset_hash
                   AND a.analyzer_version=r.visual_analyzer_version
                   AND a.supported=1),
                r.source_profile_json, r.card_description
         FROM matched m JOIN readings r ON r.rowid=m.rowid
         WHERE {view_clause}
           AND (?3 = '' OR EXISTS (SELECT 1 FROM json_each(r.tags_json) WHERE value = ?3))
           AND (?4 = '' OR r.saved_at >= ?4)
           AND (?5 = '' OR r.saved_at <= ?5)
           AND (?6 = 0 OR r.rating = ?6)
           AND (?7 = '' OR r.kind = ?7)
           AND (?8 = '' OR r.predominant_color = ?8)
           AND (?11 = '' OR (
                (
                    EXISTS (
                        SELECT 1 FROM visual_analysis a
                        WHERE a.content_hash=r.visual_asset_hash
                          AND a.analyzer_version=r.visual_analyzer_version
                          AND a.supported=1
                          AND a.visual_terms=r.visual_terms
                    )
                    AND r.rowid IN (
                        SELECT rowid FROM readings_fts WHERE readings_fts MATCH ?11
                    )
                )
                OR r.id IN (SELECT value FROM json_each(?13))
           ))
           AND NOT EXISTS (
                SELECT 1 FROM json_each(?12) requested_tag
                WHERE NOT EXISTS (
                    SELECT 1 FROM json_each(r.tags_json) reading_tag
                    WHERE reading_tag.value = requested_tag.value
                )
           )
           AND (?14 = '' OR r.id IN (SELECT value FROM json_each(?14)))
           AND NOT EXISTS (
               SELECT 1 FROM json_each(?15) requested_type
               WHERE NOT (
                   (r.kind = requested_type.value AND requested_type.value IN ('image', 'video', 'quote'))
                   OR (r.kind = 'article' AND requested_type.value = 'article'
                       AND r.lightweight = 0
                       AND json_extract(r.source_profile_json, '$.source_type') IS NOT 'social_post')
                   OR (r.kind = 'article' AND requested_type.value = 'link'
                       AND (r.lightweight = 1
                            OR json_extract(r.source_profile_json, '$.source_type') IS 'social_post'))
               )
           )
         ORDER BY {order}
         LIMIT ?1 OFFSET ?2"
    );

    let mut stmt = conn.prepare(&sql)?;
    let tag_val = opts.tag.as_deref().unwrap_or("");
    let since_val = opts.since.as_deref().unwrap_or("");
    let until_val = opts.until.as_deref().unwrap_or("");
    let rating_val = opts.rating.unwrap_or(0) as i64;
    let kind_val = opts.kind.map(ReadingKind::as_str).unwrap_or("");
    let color_val = opts
        .predominant_color
        .map(PredominantColor::as_str)
        .unwrap_or("");

    let rows = stmt.query_map(
        params![
            opts.limit as i64,
            opts.offset as i64,
            tag_val,
            since_val,
            until_val,
            rating_val,
            kind_val,
            color_val,
            match_query.unwrap_or("\"__oia_no_text_match__\""),
            semantic_json,
            crate::search::scoped_visual_query(&opts.visual_terms).unwrap_or_default(),
            serde_json::to_string(&opts.tag_terms)?,
            serde_json::to_string(&opts.visual_semantic_candidate_ids)?,
            color_ids_json,
            serde_json::to_string(&opts.item_type_terms)?,
        ],
        parse_row,
    )?;

    rows.map(|r| r.map_err(Into::into)).collect()
}

fn parse_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReadingRow> {
    let tags_json: String = row.get(13)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let palette_json: Option<String> = row.get(24)?;
    let kind = parse_kind(row.get::<_, String>(16)?.as_str())?;
    let lightweight = row.get::<_, i32>(21)? != 0;
    let source_profile_json: Option<String> = row.get(25)?;
    let is_link = kind == ReadingKind::Article
        && (lightweight || is_social_post(source_profile_json.as_deref()));
    let dominant_color = palette_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<WeightedColor>>(json).ok())
        .and_then(|palette| crate::visual_index::representative_placeholder_color(&palette));
    Ok(ReadingRow {
        id: row.get(0)?,
        title: row.get(1)?,
        kind,
        lightweight,
        is_link,
        has_note: row.get::<_, i32>(22)? != 0,
        url: row.get(2)?,
        media_url: row.get(17)?,
        preview_asset: row.get(18)?,
        favicon_asset: row.get(19)?,
        theme_color: row.get(20)?,
        source_profile_json,
        dominant_color,
        media_aspect_ratio: row.get(23)?,
        canonical_url: row.get(3)?,
        author: row.get(4)?,
        site: row.get(5)?,
        saved_at: row.get(6)?,
        read: row.get::<_, i32>(7)? != 0,
        archived: row.get::<_, i32>(8)? != 0,
        favorite: row.get::<_, i32>(9)? != 0,
        excerpt: row.get(10)?,
        card_description: row.get(26)?,
        word_count: row.get(11)?,
        lang: row.get(12)?,
        tags,
        rating: row.get::<_, i32>(14)? as u8,
        read_at: row.get(15)?,
    })
}

fn is_social_post(profile_json: Option<&str>) -> bool {
    profile_json
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|profile| {
            profile
                .get("source_type")
                .and_then(serde_json::Value::as_str)
                .map(|value| value == "social_post")
        })
        .unwrap_or(false)
}

fn parse_kind(value: &str) -> rusqlite::Result<ReadingKind> {
    match value {
        "article" => Ok(ReadingKind::Article),
        "image" => Ok(ReadingKind::Image),
        "video" => Ok(ReadingKind::Video),
        "quote" => Ok(ReadingKind::Quote),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            16,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid reading kind: {other}"),
            )),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        index::open, new_id, reconcile::rebuild, write_reading, LibraryRoot, Metadata,
        SourceAttachment, SourceProfile,
    };

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

    fn portrait_png() -> Vec<u8> {
        let mut bytes = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        bytes.extend_from_slice(&1900_u32.to_be_bytes());
        bytes.extend_from_slice(&2468_u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn attach_visual_analysis(conn: &Connection, id: &str, visual_terms: &str) {
        conn.execute(
            "INSERT INTO visual_analysis
                 (content_hash, analyzer_version, supported, labels_json, palette_json,
                  visual_terms)
             VALUES (?1, 'structured-search-test', 1, '[]', '[]', ?2)",
            params![id, visual_terms],
        )
        .unwrap();
        conn.execute(
            "UPDATE readings
             SET visual_asset_hash=?1,
                 visual_analyzer_version='structured-search-test',
                 visual_terms=?2
             WHERE id=?1",
            params![id, visual_terms],
        )
        .unwrap();
    }

    #[test]
    fn rebuild_projects_source_profile_json_through_list_and_ffi() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let mut metadata = meta(&new_id(), "https://x.com/example/status/42", "A post");
        metadata.source_profile = Some(SourceProfile {
            version: 1,
            source_type: "future-social-kind".into(),
            provider: "future-provider".into(),
            source_id: "42".into(),
            author_handle: "@example".into(),
            published_at: None,
            avatar_asset: None,
            attachments: vec![SourceAttachment {
                kind: "future-attachment-kind".into(),
                asset: "assets/attachment.bin".into(),
                poster_asset: None,
                content_type: Some("application/octet-stream".into()),
                width: None,
                height: None,
                alt: None,
            }],
        });
        write_reading(&lib, metadata, "Post text".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let row = list_readings(&conn, &ListOptions::default())
            .unwrap()
            .pop()
            .unwrap();
        let json = row.source_profile_json.clone().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["source_type"], "future-social-kind");
        assert_eq!(value["provider"], "future-provider");
        assert_eq!(value["attachments"][0]["kind"], "future-attachment-kind");
        assert_eq!(value["attachments"][0]["asset"], "assets/attachment.bin");

        let ffi_row: crate::ffi::FfiReadingRow = row.into();
        assert_eq!(ffi_row.source_profile_json.as_deref(), Some(json.as_str()));
    }

    #[test]
    fn article_card_projection_flows_through_listing_search_and_get() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        let mut metadata = meta(&id, "https://example.com/essay", "The heading");
        metadata.excerpt = Some("The heading.".into());
        write_reading(
            &lib,
            metadata,
            "# The heading\n\nOpening saved prose explains the article.".into(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        let listed = list_readings(&conn, &ListOptions::default()).unwrap();
        assert_eq!(listed.len(), 1);
        let row = &listed[0];
        assert_eq!(row.excerpt.as_deref(), Some("The heading."));
        assert_eq!(
            row.card_description.as_deref(),
            Some("Opening saved prose explains the article.")
        );
        assert_eq!(row.word_count, Some(8));

        let searched = list_readings(
            &conn,
            &ListOptions {
                query: Some("Opening".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(searched[0].card_description, row.card_description);

        let (fetched, body) = get_reading(&conn, &id).unwrap().unwrap();
        assert_eq!(fetched.card_description, row.card_description);
        assert!(body.starts_with("# The heading"));
        let ffi: crate::ffi::FfiReadingRow = fetched.into();
        assert_eq!(ffi.card_description, row.card_description);

        let saved =
            crate::parse_reading(&fs::read_to_string(lib.article_path(&id)).unwrap()).unwrap();
        assert_eq!(saved.metadata.excerpt.as_deref(), Some("The heading."));
        assert_eq!(saved.metadata.word_count, None);
    }

    #[test]
    fn list_query_filters_by_fulltext() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        write_reading(
            &lib,
            meta(&new_id(), "https://a.com", "Rust async"),
            "learning rust async".into(),
        )
        .unwrap();
        write_reading(
            &lib,
            meta(&new_id(), "https://b.com", "Cooking"),
            "pasta recipe".into(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                query: Some("rust".into()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Rust async");
    }

    #[test]
    fn search_finds_a_reading_sharing_a_bucket_with_others() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        // Two readings whose ids share one fan-out bucket, with distinct content.
        let a = "ab00000000000000000000000000000000000000000000000000000000000001";
        let b = "ab00000000000000000000000000000000000000000000000000000000000002";
        write_reading(
            &lib,
            meta(a, "https://a.com", "Rust async"),
            "learning rust async".into(),
        )
        .unwrap();
        write_reading(
            &lib,
            meta(b, "https://b.com", "Cooking"),
            "a pasta recipe".into(),
        )
        .unwrap();
        assert_eq!(
            lib.reading_dir(a).parent().unwrap(),
            lib.reading_dir(b).parent().unwrap(),
            "the two readings must share one bucket"
        );
        rebuild(&conn, &lib).unwrap();

        // Search resolves to exactly the matching bucket sibling.
        let rows = list_readings(
            &conn,
            &ListOptions {
                query: Some("pasta".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1, "only one reading matches");
        assert_eq!(rows[0].id, b);
        assert_eq!(rows[0].title, "Cooking");
    }

    #[test]
    fn list_blank_query_matches_nothing() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        write_reading(&lib, meta(&new_id(), "https://a.com", "A"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // A present-but-untokenizable query is still a search, so it returns no
        // rows rather than falling back to the full listing.
        let rows = list_readings(
            &conn,
            &ListOptions {
                query: Some("   ".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn list_all_includes_legacy_archived_readings() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        write_reading(&lib, meta(&new_id(), "https://a.com", "A"), "body".into()).unwrap();
        let mut m = meta(&new_id(), "https://b.com", "B");
        m.archived = true;
        write_reading(&lib, m, "body".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::All,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.title == "A"));
        assert!(rows.iter().any(|row| row.title == "B" && row.archived));
    }

    #[test]
    fn list_unread_excludes_read_and_archived() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        write_reading(
            &lib,
            meta(&new_id(), "https://a.com", "Unread"),
            "body".into(),
        )
        .unwrap();

        let mut read_meta = meta(&new_id(), "https://b.com", "Read");
        read_meta.read_at = Some("2026-06-13T16:00:00.000Z".to_string());
        write_reading(&lib, read_meta, "body".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::Unread,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Unread");
    }

    #[test]
    fn list_read_returns_read_non_archived() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        write_reading(
            &lib,
            meta(&new_id(), "https://a.com", "Unread"),
            "body".into(),
        )
        .unwrap();

        let mut read_meta = meta(&new_id(), "https://b.com", "Read");
        read_meta.read_at = Some("2026-06-13T16:00:00.000Z".to_string());
        write_reading(&lib, read_meta, "body".into()).unwrap();

        // A read but archived item must not appear under the Read view.
        let mut read_archived = meta(&new_id(), "https://c.com", "ReadArchived");
        read_archived.read_at = Some("2026-06-13T16:00:00.000Z".to_string());
        read_archived.archived = true;
        write_reading(&lib, read_archived, "body".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::Read,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Read");
    }

    #[test]
    fn list_archive_returns_only_archived() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        write_reading(
            &lib,
            meta(&new_id(), "https://a.com", "Active"),
            "body".into(),
        )
        .unwrap();
        let mut m = meta(&new_id(), "https://b.com", "Archived");
        m.archived = true;
        write_reading(&lib, m, "body".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::Archive,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Archived");
    }

    #[test]
    fn list_favorites() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut m = meta(&new_id(), "https://a.com", "Fav");
        m.favorite = true;
        write_reading(&lib, m, "body".into()).unwrap();
        write_reading(
            &lib,
            meta(&new_id(), "https://b.com", "Normal"),
            "body".into(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::Favorites,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Fav");
    }

    #[test]
    fn view_counts_match_list_lengths() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        // A: active, unread
        write_reading(&lib, meta(&new_id(), "https://a.com", "A"), "b".into()).unwrap();
        // B: active, read
        let mut b = meta(&new_id(), "https://b.com", "B");
        b.read_at = Some("2026-06-13T16:00:00.000Z".into());
        write_reading(&lib, b, "b".into()).unwrap();
        // C: archived
        let mut c = meta(&new_id(), "https://c.com", "C");
        c.archived = true;
        write_reading(&lib, c, "b".into()).unwrap();
        // D: active, unread, favorite
        let mut d = meta(&new_id(), "https://d.com", "D");
        d.favorite = true;
        write_reading(&lib, d, "b".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        let counts = view_counts(&conn, &CountScope::default()).unwrap();
        assert_eq!(counts.all, 4); // All includes legacy archived rows.
        assert_eq!(counts.unread, 2); // A, D
        assert_eq!(counts.read, 1); // B
        assert_eq!(counts.archive, 1); // C
        assert_eq!(counts.favorites, 1); // D

        // Each grouped count must equal the length of the corresponding list —
        // the one-pass query agrees with the old materialize-and-count.
        let len = |view| {
            list_readings(
                &conn,
                &ListOptions {
                    view,
                    limit: 9999,
                    ..Default::default()
                },
            )
            .unwrap()
            .len() as u64
        };
        assert_eq!(counts.all, len(View::All));
        assert_eq!(counts.unread, len(View::Unread));
        assert_eq!(counts.read, len(View::Read));
        assert_eq!(counts.archive, len(View::Archive));
        assert_eq!(counts.favorites, len(View::Favorites));
    }

    #[test]
    fn view_counts_zero_on_empty_library() {
        let (_dir, conn) = setup();
        // FILTER yields 0, not NULL, so every field is a clean zero.
        assert_eq!(
            view_counts(&conn, &CountScope::default()).unwrap(),
            ViewCounts::default()
        );
    }

    /// Build the four-reading faceting corpus shared by the scoped view-count
    /// tests. "space" appears in X, Y, W but not Z; ratings and archived state
    /// are spread so search/tag/rating facets each narrow differently.
    fn faceting_corpus(lib: &LibraryRoot, conn: &Connection) {
        // X: active, unread, tag "sci", rating 5.
        let mut x = meta(&new_id(), "https://a.com", "X");
        x.tags = vec!["sci".into()];
        x.rating = 5;
        write_reading(lib, x, "space opera".into()).unwrap();
        // Y: active, read, tag "sci", rating 3.
        let mut y = meta(&new_id(), "https://b.com", "Y");
        y.tags = vec!["sci".into()];
        y.rating = 3;
        y.read_at = Some("2026-06-13T16:00:00.000Z".into());
        write_reading(lib, y, "space station".into()).unwrap();
        // Z: active, unread, tag "cook", rating 2 — never matches "space".
        let mut z = meta(&new_id(), "https://c.com", "Z");
        z.tags = vec!["cook".into()];
        z.rating = 2;
        write_reading(lib, z, "pasta recipe".into()).unwrap();
        // W: archived, tag "sci", rating 5.
        let mut w = meta(&new_id(), "https://d.com", "W");
        w.tags = vec!["sci".into()];
        w.rating = 5;
        w.archived = true;
        write_reading(lib, w, "space archived".into()).unwrap();
        rebuild(conn, lib).unwrap();
    }

    #[test]
    fn view_counts_scoped_by_search() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // "space" matches X (active unread), Y (active read), W (archived); Z is out.
        let counts = view_counts(
            &conn,
            &CountScope {
                query: Some("space".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(counts.all, 3, "X, Y, W (legacy archive ignored by All)");
        assert_eq!(counts.unread, 1, "X");
        assert_eq!(counts.read, 1, "Y");
        assert_eq!(counts.archive, 1, "W");
        assert_eq!(counts.favorites, 0);
    }

    #[test]
    fn view_counts_scoped_by_rating_facet_composes_with_search() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // Selecting 5★ narrows the "space" set to X (active) and W (archived);
        // Y is rating 3 and drops out. The view axis is never applied to itself.
        let counts = view_counts(
            &conn,
            &CountScope {
                query: Some("space".into()),
                rating: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(counts.all, 2, "X, W (Y is 3★)");
        assert_eq!(counts.unread, 1, "X");
        assert_eq!(counts.read, 0, "Y dropped by the 5★ facet");
        assert_eq!(counts.archive, 1, "W");
    }

    #[test]
    fn view_counts_scoped_by_tag_facet() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // The "cook" tag matches only Z (active, unread) — no search.
        let counts = view_counts(
            &conn,
            &CountScope {
                tag: Some("cook".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(counts.all, 1, "Z");
        assert_eq!(counts.unread, 1, "Z");
        assert_eq!(counts.read, 0);
        assert_eq!(counts.archive, 0);
    }

    #[test]
    fn view_counts_unmatchable_search_is_all_zero() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // A real word that matches nothing (Some(match) but no rows) and a
        // blank/punctuation query (early None) both scope every view to zero.
        for q in ["zzzznomatch", "   "] {
            let counts = view_counts(
                &conn,
                &CountScope {
                    query: Some(q.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(counts, ViewCounts::default(), "query {q:?} matches nothing");
        }
    }

    #[test]
    fn sidebar_counts_matches_the_individual_sections() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // The batched call resolves the search once, but every section must return
        // exactly what its standalone function does — across plain, searched,
        // faceted, view-scoped, and unmatchable-search scopes. This ties the
        // batched path to the individual functions' behavior tests.
        let scopes = [
            CountScope::default(),
            CountScope {
                query: Some("space".into()),
                ..Default::default()
            },
            CountScope {
                query: Some("space".into()),
                rating: Some(5),
                ..Default::default()
            },
            CountScope {
                view: View::Unread,
                tag: Some("sci".into()),
                ..Default::default()
            },
            CountScope {
                query: Some("zzzznomatch".into()),
                ..Default::default()
            },
        ];
        for scope in scopes {
            let batched = sidebar_counts(&conn, &scope).unwrap();
            assert_eq!(
                batched.views,
                view_counts(&conn, &scope).unwrap(),
                "views for {scope:?}"
            );
            assert_eq!(
                batched.tags,
                crate::list_tags(&conn, &scope).unwrap(),
                "tags for {scope:?}"
            );
            assert_eq!(
                batched.ratings,
                crate::list_ratings(&conn, &scope).unwrap(),
                "ratings for {scope:?}"
            );
        }
    }

    #[test]
    fn list_filter_by_tag() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut m = meta(&new_id(), "https://a.com", "Tagged");
        m.tags = vec!["rust".into()];
        write_reading(&lib, m, "body".into()).unwrap();
        write_reading(
            &lib,
            meta(&new_id(), "https://b.com", "Untagged"),
            "body".into(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::All,
                tag: Some("rust".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Tagged");
    }

    #[test]
    fn list_filter_by_rating() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut four = meta(&new_id(), "https://a.com", "Four");
        four.rating = 4;
        write_reading(&lib, four, "body".into()).unwrap();

        let mut five = meta(&new_id(), "https://b.com", "Five");
        five.rating = 5;
        write_reading(&lib, five, "body".into()).unwrap();

        write_reading(
            &lib,
            meta(&new_id(), "https://c.com", "Unrated"),
            "body".into(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::All,
                rating: Some(4),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Four");
        assert_eq!(rows[0].rating, 4);
    }

    #[test]
    fn list_limit_and_offset() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        for i in 0..5u32 {
            write_reading(
                &lib,
                meta(
                    &new_id(),
                    &format!("https://example.com/{i}"),
                    &format!("Article {i}"),
                ),
                "body".into(),
            )
            .unwrap();
        }
        rebuild(&conn, &lib).unwrap();

        let page1 = list_readings(
            &conn,
            &ListOptions {
                view: View::All,
                limit: 3,
                offset: 0,
                ..Default::default()
            },
        )
        .unwrap();
        let page2 = list_readings(
            &conn,
            &ListOptions {
                view: View::All,
                limit: 3,
                offset: 3,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(page1.len(), 3);
        assert_eq!(page2.len(), 2);
        // No overlap.
        let ids1: Vec<_> = page1.iter().map(|r| &r.id).collect();
        let ids2: Vec<_> = page2.iter().map(|r| &r.id).collect();
        assert!(ids1.iter().all(|id| !ids2.contains(id)));
    }

    #[test]
    fn sort_by_read_at_puts_unread_last_in_both_directions() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut a = meta(&new_id(), "https://a.com", "A");
        a.read_at = Some("2026-01-02T00:00:00.000Z".into());
        write_reading(&lib, a, "body".into()).unwrap();

        let mut b = meta(&new_id(), "https://b.com", "B");
        b.read_at = Some("2026-01-03T00:00:00.000Z".into());
        write_reading(&lib, b, "body".into()).unwrap();

        write_reading(
            &lib,
            meta(&new_id(), "https://c.com", "C-unread"),
            "body".into(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();

        let titles = |ascending: bool| {
            list_readings(
                &conn,
                &ListOptions {
                    view: View::All,
                    sort: SortField::ReadAt,
                    ascending,
                    ..Default::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|r| r.title)
            .collect::<Vec<_>>()
        };

        // Descending: most recently read first; unread last.
        assert_eq!(titles(false), ["B", "A", "C-unread"]);
        // Ascending: earliest read first; unread STILL last.
        assert_eq!(titles(true), ["A", "B", "C-unread"]);
    }

    #[test]
    fn sort_by_rating_orders_by_stars() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut five = meta(&new_id(), "https://a.com", "Five");
        five.rating = 5;
        write_reading(&lib, five, "body".into()).unwrap();

        let mut three = meta(&new_id(), "https://b.com", "Three");
        three.rating = 3;
        write_reading(&lib, three, "body".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::All,
                sort: SortField::Rating,
                ascending: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            ["Five", "Three"]
        );
    }

    #[test]
    fn sort_by_word_count_puts_unknown_last_in_both_directions() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut long = meta(&new_id(), "https://a.com", "Long");
        long.word_count = Some(5000);
        write_reading(&lib, long, "body".into()).unwrap();

        let mut short = meta(&new_id(), "https://b.com", "Short");
        short.word_count = Some(200);
        write_reading(&lib, short, "body".into()).unwrap();

        // No saved count or body text to derive one -> ranks last.
        write_reading(
            &lib,
            meta(&new_id(), "https://c.com", "Unknown"),
            String::new(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();

        let titles = |ascending: bool| {
            list_readings(
                &conn,
                &ListOptions {
                    view: View::All,
                    sort: SortField::WordCount,
                    ascending,
                    ..Default::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|r| r.title)
            .collect::<Vec<_>>()
        };

        // Descending: longest read first; unknown last.
        assert_eq!(titles(false), ["Long", "Short", "Unknown"]);
        // Ascending: shortest read first; unknown STILL last.
        assert_eq!(titles(true), ["Short", "Long", "Unknown"]);
    }

    #[test]
    fn get_reading_returns_body() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com", "My Article"),
            "hello world".into(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        let result = get_reading(&conn, &id).unwrap();
        assert!(result.is_some());
        let (row, body) = result.unwrap();
        assert_eq!(row.title, "My Article");
        assert!(body.contains("hello world"));
    }

    #[test]
    fn list_row_carries_media_metadata() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        let mut metadata = meta(&id, "https://example.com/gallery", "Photo");
        metadata.kind = ReadingKind::Image;
        metadata.media_url = Some("https://cdn.example.com/photo.jpg".into());
        metadata.preview_asset = Some("assets/photo.png".into());
        write_reading(&lib, metadata, "![Photo](assets/photo.png)".into()).unwrap();
        fs::create_dir_all(lib.assets_dir(&id)).unwrap();
        fs::write(lib.assets_dir(&id).join("photo.png"), portrait_png()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let row = list_readings(&conn, &ListOptions::default())
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(row.kind, ReadingKind::Image);
        assert_eq!(
            row.media_url.as_deref(),
            Some("https://cdn.example.com/photo.jpg")
        );
        assert_eq!(row.preview_asset.as_deref(), Some("assets/photo.png"));
        assert_eq!(row.dominant_color, None);
        assert_eq!(row.media_aspect_ratio, Some(1900.0 / 2468.0));

        let search_row = list_readings(
            &conn,
            &ListOptions {
                query: Some("Photo".into()),
                ..ListOptions::default()
            },
        )
        .unwrap()
        .pop()
        .unwrap();
        assert_eq!(search_row.media_aspect_ratio, row.media_aspect_ratio);

        let (single_row, _) = get_reading(&conn, &id).unwrap().unwrap();
        assert_eq!(single_row.media_aspect_ratio, row.media_aspect_ratio);
    }

    #[test]
    fn list_row_carries_article_preview_aspect_ratio() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        let mut metadata = meta(&id, "https://example.com/article", "Article");
        metadata.preview_asset = Some("assets/social.png".into());
        write_reading(&lib, metadata, "Article body".into()).unwrap();
        fs::create_dir_all(lib.assets_dir(&id)).unwrap();
        fs::write(lib.assets_dir(&id).join("social.png"), portrait_png()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let row = list_readings(&conn, &ListOptions::default())
            .unwrap()
            .pop()
            .unwrap();

        assert_eq!(row.kind, ReadingKind::Article);
        assert_eq!(row.media_aspect_ratio, Some(1900.0 / 2468.0));
    }

    #[test]
    fn kind_filter_is_applied_before_pagination() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        for (title, kind) in [
            ("Article", ReadingKind::Article),
            ("Image A", ReadingKind::Image),
            ("Quote", ReadingKind::Quote),
            ("Image B", ReadingKind::Image),
        ] {
            let id = new_id();
            let mut metadata = meta(&id, &format!("https://example.com/{id}"), title);
            metadata.kind = kind;
            write_reading(&lib, metadata, "shared body".into()).unwrap();
        }
        rebuild(&conn, &lib).unwrap();

        let page = |offset| {
            list_readings(
                &conn,
                &ListOptions {
                    kind: Some(ReadingKind::Image),
                    limit: 1,
                    offset,
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let first = page(0);
        let second = page(1);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert!(page(2).is_empty());
        assert_eq!(first[0].kind, ReadingKind::Image);
        assert_eq!(second[0].kind, ReadingKind::Image);
        assert_ne!(first[0].id, second[0].id);
    }

    #[test]
    fn quote_kind_filter_composes_with_full_text_search() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        for (title, kind) in [
            ("Article", ReadingKind::Article),
            ("Saved quote", ReadingKind::Quote),
        ] {
            let id = new_id();
            let mut metadata = meta(&id, &format!("https://example.com/{id}"), title);
            metadata.kind = kind;
            write_reading(&lib, metadata, "shared searchable phrase".into()).unwrap();
        }
        rebuild(&conn, &lib).unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                kind: Some(ReadingKind::Quote),
                query: Some("searchable".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, ReadingKind::Quote);
    }

    #[test]
    fn phrase_fallback_is_resolved_inside_the_active_view() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let image_id = new_id();
        let mut image = meta(&image_id, "https://example.com/exact-image", "Exact image");
        image.kind = ReadingKind::Image;
        image.tags = vec!["visual".into()];
        image.rating = 5;
        write_reading(&lib, image, "alpha beta appears together".into()).unwrap();

        let quote_id = new_id();
        let mut quote = meta(
            &quote_id,
            "https://example.com/separated-quote",
            "Separated quote",
        );
        quote.kind = ReadingKind::Quote;
        quote.tags = vec!["words".into()];
        quote.rating = 4;
        write_reading(&lib, quote, "alpha appears far before beta".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        // The image is an exact phrase hit, but it is outside Quotes. Search
        // therefore falls back to all-words-AND inside the active view and keeps
        // the non-contiguous quote visible.
        let rows = list_readings(
            &conn,
            &ListOptions {
                view: View::Quotes,
                query: Some("alpha beta".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, quote_id);

        // Batched toolbar counts resolve the same scoped fallback as the list.
        let counts = sidebar_counts(
            &conn,
            &CountScope {
                view: View::Quotes,
                query: Some("alpha beta".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(counts.tags, vec![("visual".into(), 0), ("words".into(), 1)]);
        assert_eq!(counts.ratings, vec![(5, 0), (4, 1)]);
    }

    #[test]
    fn pagination_does_not_influence_phrase_fallback() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let exact_id = new_id();
        let mut exact = meta(&exact_id, "https://example.com/exact", "Exact quote");
        exact.kind = ReadingKind::Quote;
        write_reading(&lib, exact, "alpha beta appears together".into()).unwrap();

        let separated_id = new_id();
        let mut separated = meta(
            &separated_id,
            "https://example.com/separated",
            "Separated quote",
        );
        separated.kind = ReadingKind::Quote;
        write_reading(&lib, separated, "alpha appears far before beta".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        let all_matches = list_readings(
            &conn,
            &ListOptions {
                view: View::Quotes,
                query: Some("alpha beta".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(all_matches.len(), 1);
        assert_eq!(all_matches[0].id, exact_id);

        // The scoped result set has only one phrase hit. Page two must therefore
        // stay empty instead of switching to AND merely because the phrase hit
        // was consumed by the first page.
        let second_page = list_readings(
            &conn,
            &ListOptions {
                view: View::Quotes,
                query: Some("alpha beta".into()),
                limit: 1,
                offset: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(second_page.is_empty());
    }

    #[test]
    fn sidebar_counts_compose_with_kind() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        for (title, kind, tag, rating) in [
            ("Photo", ReadingKind::Image, "visual", 5),
            ("Quote", ReadingKind::Quote, "words", 4),
            ("Article", ReadingKind::Article, "visual", 5),
        ] {
            let id = new_id();
            let mut metadata = meta(&id, &format!("https://example.com/{id}"), title);
            metadata.kind = kind;
            metadata.tags = vec![tag.into()];
            metadata.rating = rating;
            write_reading(&lib, metadata, "body".into()).unwrap();
        }
        rebuild(&conn, &lib).unwrap();

        let counts = sidebar_counts(
            &conn,
            &CountScope {
                kind: Some(ReadingKind::Image),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(counts.views.all, 1);
        assert_eq!(counts.views.unread, 1);
        assert_eq!(counts.tags, vec![("visual".into(), 1), ("words".into(), 0)]);
        assert_eq!(counts.ratings, vec![(5, 1), (4, 0)]);
    }

    #[test]
    fn board_views_filter_derived_card_categories_before_pagination() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let article_id = new_id();
        write_reading(
            &lib,
            meta(&article_id, "https://x.com/example/status/43", "Article"),
            "article body".into(),
        )
        .unwrap();

        let link_id = new_id();
        let mut link = meta(&link_id, "https://example.com/link", "Link");
        link.lightweight = true;
        write_reading(&lib, link, "[Open link](https://example.com/link)".into()).unwrap();

        let post_id = new_id();
        let mut post = meta(&post_id, "https://x.com/example/status/42", "Tweet");
        post.source_profile = Some(SourceProfile {
            version: 1,
            source_type: "social_post".into(),
            provider: "x".into(),
            source_id: "42".into(),
            author_handle: "example".into(),
            published_at: None,
            avatar_asset: None,
            attachments: vec![],
        });
        write_reading(&lib, post, "Locally captured post text".into()).unwrap();

        let image_id = new_id();
        let mut image = meta(&image_id, "https://example.com/image", "Image");
        image.kind = ReadingKind::Image;
        write_reading(&lib, image, "image body".into()).unwrap();
        fs::write(lib.note_path(&image_id), "Why this image matters").unwrap();

        let video_id = new_id();
        let mut video = meta(&video_id, "https://example.com/video", "Video");
        video.kind = ReadingKind::Video;
        write_reading(&lib, video, "video body".into()).unwrap();

        let quote_id = new_id();
        let mut quote = meta(&quote_id, "https://example.com/quote", "Quote");
        quote.kind = ReadingKind::Quote;
        quote.favorite = true;
        write_reading(&lib, quote, "> quoted text".into()).unwrap();

        rebuild(&conn, &lib).unwrap();

        let titles = |view| {
            list_readings(
                &conn,
                &ListOptions {
                    view,
                    limit: 20,
                    ..Default::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|row| row.title)
            .collect::<std::collections::BTreeSet<_>>()
        };

        assert_eq!(titles(View::All).len(), 6);
        assert_eq!(titles(View::Favorites), ["Quote".into()].into());
        assert_eq!(titles(View::Media), ["Image".into(), "Video".into()].into());
        assert_eq!(titles(View::Articles), ["Article".into()].into());
        assert_eq!(titles(View::Notes), ["Image".into()].into());
        assert_eq!(titles(View::Links), ["Link".into(), "Tweet".into()].into());
        assert_eq!(titles(View::Quotes), ["Quote".into()].into());

        let media_page = |offset| {
            list_readings(
                &conn,
                &ListOptions {
                    view: View::Media,
                    limit: 1,
                    offset,
                    ..Default::default()
                },
            )
            .unwrap()
        };
        assert_eq!(media_page(0).len(), 1);
        assert_eq!(media_page(1).len(), 1);
        assert!(media_page(2).is_empty());

        let rows = list_readings(&conn, &ListOptions::default()).unwrap();
        let link_row = rows.iter().find(|row| row.id == link_id).unwrap();
        assert!(link_row.lightweight);
        assert!(link_row.is_link);
        let post_row = rows.iter().find(|row| row.id == post_id).unwrap();
        assert!(!post_row.lightweight);
        assert!(post_row.is_link);
        assert!(crate::ffi::FfiReadingRow::from(post_row.clone()).is_link);
        assert!(
            !rows
                .iter()
                .find(|row| row.id == article_id)
                .unwrap()
                .is_link
        );
        assert!(!link_row.has_note);
        let image_row = rows.iter().find(|row| row.id == image_id).unwrap();
        assert!(!image_row.lightweight);
        assert!(image_row.has_note);
    }

    #[test]
    fn get_reading_returns_none_for_missing_id() {
        let (_dir, conn) = setup();
        let result = get_reading(&conn, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn semantic_candidates_compose_with_text_filters_order_pagination_and_counts() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let text_id = new_id();
        write_reading(
            &lib,
            meta(&text_id, "https://example.com/text", "Text match"),
            "needle in the body".into(),
        )
        .unwrap();
        let image_id = new_id();
        let mut image = meta(&image_id, "https://example.com/image", "Semantic image");
        image.kind = ReadingKind::Image;
        image.tags = vec!["design".into()];
        write_reading(&lib, image, "no lexical overlap".into()).unwrap();
        let quote_id = new_id();
        let mut quote = meta(&quote_id, "https://example.com/quote", "Semantic quote");
        quote.kind = ReadingKind::Quote;
        quote.tags = vec!["design".into()];
        write_reading(&lib, quote, "another unrelated body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let candidates = vec![
            quote_id.clone(),
            image_id.clone(),
            text_id.clone(),
            quote_id.clone(),
        ];
        let rows = list_readings(
            &conn,
            &ListOptions {
                sort: SortField::Relevance,
                query: Some("needle".into()),
                semantic_candidate_ids: candidates.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            [text_id.as_str(), quote_id.as_str(), image_id.as_str()]
        );

        let page = list_readings(
            &conn,
            &ListOptions {
                sort: SortField::Relevance,
                query: Some("needle".into()),
                semantic_candidate_ids: candidates.clone(),
                limit: 1,
                offset: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page[0].id, quote_id);

        let images = list_readings(
            &conn,
            &ListOptions {
                query: Some("needle".into()),
                kind: Some(ReadingKind::Image),
                tag: Some("design".into()),
                semantic_candidate_ids: candidates.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].id, image_id);

        assert_eq!(
            view_counts(
                &conn,
                &CountScope {
                    query: Some("needle".into()),
                    semantic_candidate_ids: candidates.clone(),
                    ..Default::default()
                }
            )
            .unwrap()
            .all,
            3
        );
        assert_eq!(
            view_counts(
                &conn,
                &CountScope {
                    query: Some("needle".into()),
                    kind: Some(ReadingKind::Image),
                    tag: Some("design".into()),
                    semantic_candidate_ids: candidates,
                    ..Default::default()
                }
            )
            .unwrap()
            .all,
            1
        );
    }

    #[test]
    fn multiword_visual_fallback_survives_a_text_phrase_hit() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let text_id = new_id();
        write_reading(
            &lib,
            meta(&text_id, "https://example.com/text", "Text"),
            "a blue chair is here".into(),
        )
        .unwrap();
        let visual_id = new_id();
        write_reading(
            &lib,
            meta(&visual_id, "https://example.com/visual", "Visual"),
            "unrelated".into(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();
        conn.execute(
            "UPDATE readings SET visual_terms='blue furniture chair' WHERE id=?1",
            params![visual_id],
        )
        .unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                query: Some("blue chair".into()),
                sort: SortField::Relevance,
                ..Default::default()
            },
        )
        .unwrap();
        let ids: std::collections::HashSet<_> = rows.into_iter().map(|row| row.id).collect();
        assert_eq!(ids, [text_id, visual_id].into());
    }

    #[test]
    fn visual_terms_match_the_same_analysis_without_text_or_tag_false_positives() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let matching_id = new_id();
        let mut matching = meta(
            &matching_id,
            "https://example.com/matching",
            "Matching image",
        );
        matching.kind = ReadingKind::Image;
        write_reading(&lib, matching, "unrelated".into()).unwrap();

        let blue_only_id = new_id();
        let mut blue_only = meta(&blue_only_id, "https://example.com/blue", "Blue image");
        blue_only.kind = ReadingKind::Image;
        write_reading(&lib, blue_only, "unrelated".into()).unwrap();

        let furniture_only_id = new_id();
        let mut furniture_only = meta(
            &furniture_only_id,
            "https://example.com/furniture",
            "Furniture image",
        );
        furniture_only.kind = ReadingKind::Image;
        write_reading(&lib, furniture_only, "unrelated".into()).unwrap();

        let text_only_id = new_id();
        write_reading(
            &lib,
            meta(
                &text_only_id,
                "https://example.com/text",
                "Blue furniture in words",
            ),
            "blue furniture".into(),
        )
        .unwrap();

        let tag_only_id = new_id();
        let mut tag_only = meta(&tag_only_id, "https://example.com/tags", "Tagged only");
        tag_only.tags = vec!["blue".into(), "furniture".into()];
        write_reading(&lib, tag_only, "unrelated".into()).unwrap();

        let stale_analysis_id = new_id();
        let mut stale_analysis = meta(
            &stale_analysis_id,
            "https://example.com/stale",
            "Stale analysis",
        );
        stale_analysis.kind = ReadingKind::Image;
        write_reading(&lib, stale_analysis, "unrelated".into()).unwrap();

        rebuild(&conn, &lib).unwrap();
        attach_visual_analysis(&conn, &matching_id, "blue furniture chair");
        attach_visual_analysis(&conn, &blue_only_id, "blue sky");
        attach_visual_analysis(&conn, &furniture_only_id, "furniture chair");
        attach_visual_analysis(&conn, &stale_analysis_id, "blue furniture chair");
        conn.execute(
            "UPDATE readings SET visual_analyzer_version='replacement-version' WHERE id=?1",
            params![stale_analysis_id],
        )
        .unwrap();

        let rows = list_readings(
            &conn,
            &ListOptions {
                visual_terms: vec!["blue".into(), "furniture".into()],
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            [matching_id.as_str()]
        );

        let semantic_rows = list_readings(
            &conn,
            &ListOptions {
                query: Some("semantic only".into()),
                visual_terms: vec!["blue".into(), "furniture".into()],
                semantic_candidate_ids: vec![
                    text_only_id.clone(),
                    tag_only_id.clone(),
                    blue_only_id.clone(),
                    furniture_only_id.clone(),
                    stale_analysis_id.clone(),
                    matching_id.clone(),
                ],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            semantic_rows
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            [matching_id.as_str()]
        );

        let counts = view_counts(
            &conn,
            &CountScope {
                query: Some("semantic only".into()),
                visual_terms: vec!["blue".into(), "furniture".into()],
                semantic_candidate_ids: vec![
                    text_only_id,
                    tag_only_id,
                    blue_only_id,
                    furniture_only_id,
                    stale_analysis_id,
                    matching_id,
                ],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(counts.all, 1);
    }

    #[test]
    fn visual_completion_uses_visual_candidates_not_text_metadata() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let image_content_id = new_id();
        let mut image_content = meta(
            &image_content_id,
            "https://example.com/image-content",
            "Untitled image",
        );
        image_content.kind = ReadingKind::Image;
        write_reading(&lib, image_content, "unrelated".into()).unwrap();

        let image_title_id = new_id();
        let mut image_title = meta(
            &image_title_id,
            "https://example.com/image-title",
            "Ceramics annual",
        );
        image_title.kind = ReadingKind::Image;
        write_reading(&lib, image_title, "unrelated".into()).unwrap();

        let article_text_id = new_id();
        write_reading(
            &lib,
            meta(&article_text_id, "https://example.com/article", "Article"),
            "A short history of ceramics".into(),
        )
        .unwrap();

        rebuild(&conn, &lib).unwrap();
        attach_visual_analysis(&conn, &image_content_id, "document screenshot");
        attach_visual_analysis(&conn, &image_title_id, "portrait people");

        let typed = list_readings(
            &conn,
            &ListOptions {
                query: Some("ceramics".into()),
                semantic_candidate_ids: vec![image_content_id.clone()],
                ..Default::default()
            },
        )
        .unwrap();
        let typed_ids: std::collections::HashSet<_> = typed.into_iter().map(|row| row.id).collect();
        assert_eq!(
            typed_ids,
            [
                image_content_id.clone(),
                image_title_id.clone(),
                article_text_id.clone(),
            ]
            .into()
        );

        let visual = list_readings(
            &conn,
            &ListOptions {
                visual_terms: vec!["Ceramics".into()],
                // Free-text candidates can contain metadata hits and must not
                // satisfy a structured visual token.
                semantic_candidate_ids: vec![image_title_id, article_text_id],
                // The image-only Spotlight query recognises the actual pixels.
                visual_semantic_candidate_ids: vec![image_content_id.clone()],
                ..Default::default()
            },
        )
        .unwrap();
        let visual_ids: std::collections::HashSet<_> =
            visual.into_iter().map(|row| row.id).collect();
        assert_eq!(visual_ids, [image_content_id].into());
    }

    #[test]
    fn item_type_terms_distinguish_links_and_compose_with_text() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let image_id = new_id();
        let mut image = meta(&image_id, "https://example.com/image", "Red sample");
        image.kind = ReadingKind::Image;
        write_reading(&lib, image, "red".into()).unwrap();
        let article_id = new_id();
        write_reading(
            &lib,
            meta(&article_id, "https://example.com/article", "Red sample"),
            "red".into(),
        )
        .unwrap();
        let link_id = new_id();
        let mut link = meta(&link_id, "https://example.com/link", "Red sample");
        link.lightweight = true;
        write_reading(&lib, link, "red".into()).unwrap();
        let post_id = new_id();
        let mut post = meta(&post_id, "https://x.com/example/status/42", "Red tweet");
        post.source_profile = Some(SourceProfile {
            version: 1,
            source_type: "social_post".into(),
            provider: "x".into(),
            source_id: "42".into(),
            author_handle: "example".into(),
            published_at: None,
            avatar_asset: None,
            attachments: vec![],
        });
        write_reading(&lib, post, "red post text".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let images = list_readings(
            &conn,
            &ListOptions {
                item_type_terms: vec!["image".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            images.iter().map(|row| &row.id).collect::<Vec<_>>(),
            vec![&image_id]
        );

        for (term, expected) in [("image", &image_id), ("article", &article_id)] {
            let rows = list_readings(
                &conn,
                &ListOptions {
                    query: Some("red".into()),
                    item_type_terms: vec![term.into()],
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
                vec![expected]
            );
        }

        for query in [None, Some("red")] {
            let rows = list_readings(
                &conn,
                &ListOptions {
                    query: query.map(str::to_owned),
                    item_type_terms: vec!["link".into()],
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                rows.into_iter()
                    .map(|row| row.id)
                    .collect::<std::collections::BTreeSet<_>>(),
                [link_id.clone(), post_id.clone()].into()
            );
        }
    }

    #[test]
    fn exact_tag_terms_are_conjunctive_and_compose_with_visual_terms() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let matching_id = new_id();
        let mut matching = meta(
            &matching_id,
            "https://example.com/matching",
            "Matching image",
        );
        matching.kind = ReadingKind::Image;
        matching.tags = vec!["inspiration".into(), "interiors".into()];
        write_reading(&lib, matching, "unrelated".into()).unwrap();

        let one_tag_id = new_id();
        let mut one_tag = meta(&one_tag_id, "https://example.com/one-tag", "One tag");
        one_tag.kind = ReadingKind::Image;
        one_tag.tags = vec!["inspiration".into()];
        write_reading(&lib, one_tag, "interiors appears only in text".into()).unwrap();

        let wrong_visual_id = new_id();
        let mut wrong_visual = meta(
            &wrong_visual_id,
            "https://example.com/wrong-visual",
            "Wrong visual",
        );
        wrong_visual.kind = ReadingKind::Image;
        wrong_visual.tags = vec!["inspiration".into(), "interiors".into()];
        write_reading(&lib, wrong_visual, "blue furniture in text".into()).unwrap();

        rebuild(&conn, &lib).unwrap();
        attach_visual_analysis(&conn, &matching_id, "blue furniture chair");
        attach_visual_analysis(&conn, &one_tag_id, "blue furniture chair");
        attach_visual_analysis(&conn, &wrong_visual_id, "blue sky");

        let tagged = list_readings(
            &conn,
            &ListOptions {
                tag_terms: vec!["inspiration".into(), "interiors".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tagged
                .iter()
                .map(|row| row.id.as_str())
                .collect::<std::collections::HashSet<_>>(),
            [matching_id.as_str(), wrong_visual_id.as_str()].into()
        );

        let rows = list_readings(
            &conn,
            &ListOptions {
                tag_terms: vec!["inspiration".into(), "interiors".into()],
                visual_terms: vec!["blue".into(), "furniture".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            [matching_id.as_str()]
        );

        let count_scope = CountScope {
            tag_terms: vec!["inspiration".into(), "interiors".into()],
            visual_terms: vec!["blue".into(), "furniture".into()],
            ..Default::default()
        };
        assert_eq!(view_counts(&conn, &count_scope).unwrap().all, 1);
        let counts = sidebar_counts(&conn, &count_scope).unwrap();
        assert_eq!(
            counts
                .tags
                .iter()
                .find(|(tag, _)| tag == "inspiration")
                .map(|(_, count)| *count),
            Some(1)
        );
    }

    #[test]
    fn free_text_phrase_fallback_is_resolved_inside_the_structured_scope() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let in_scope_id = new_id();
        let mut in_scope = meta(&in_scope_id, "https://example.com/in-scope", "In scope");
        in_scope.kind = ReadingKind::Image;
        write_reading(&lib, in_scope, "alpha appears before unrelated beta".into()).unwrap();

        let phrase_outside_scope_id = new_id();
        let mut phrase_outside_scope = meta(
            &phrase_outside_scope_id,
            "https://example.com/outside-scope",
            "Outside scope",
        );
        phrase_outside_scope.kind = ReadingKind::Image;
        write_reading(&lib, phrase_outside_scope, "exact alpha beta phrase".into()).unwrap();

        rebuild(&conn, &lib).unwrap();
        attach_visual_analysis(&conn, &in_scope_id, "blue furniture chair");
        attach_visual_analysis(&conn, &phrase_outside_scope_id, "red landscape");

        let options = ListOptions {
            query: Some("alpha beta".into()),
            visual_terms: vec!["blue".into(), "furniture".into()],
            ..Default::default()
        };
        let rows = list_readings(&conn, &options).unwrap();
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            [in_scope_id.as_str()]
        );

        assert_eq!(
            view_counts(
                &conn,
                &CountScope {
                    query: options.query,
                    visual_terms: options.visual_terms,
                    ..Default::default()
                }
            )
            .unwrap()
            .all,
            1
        );
    }
}
