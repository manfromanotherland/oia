// SPDX-License-Identifier: MIT

use anyhow::{bail, Result};
use rusqlite::{params_from_iter, types::Value, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_casefold::UnicodeCaseFold;

use crate::{
    list::{pinned_count_filter, CountScope, Facet, ResolvedSearch},
    locking::lock_reading,
    reconcile::apply_diffs,
    scanner::{ScanDiff, ScannedReading},
    writer::write_reading_under_lock,
    LibraryRoot, MachineTagSource, Metadata,
};

#[cfg(test)]
use crate::parse_reading;

/// The longest a tag name may be, counted in Unicode scalar values (`char`s),
/// not bytes. Enforced by [`add_tag`]; the macOS client mirrors this limit to
/// surface the error before it reaches the core.
pub const MAX_TAG_LEN: usize = 20;
const MAX_MACHINE_TAGS_PER_SOURCE: usize = 8;
const MAX_TEXT_TAGGING_CHARS: usize = 4_000;
const IMAGE_TAG_MIN_CONFIDENCE: f64 = 0.25;
const MAX_IMAGE_TAGS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagEntry {
    pub name: String,
    pub key: String,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextTaggingTask {
    pub reading_id: String,
    pub title: String,
    pub text: String,
    pub source_fingerprint: String,
    pub analyzer_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextTaggingBatch {
    pub tasks: Vec<TextTaggingTask>,
    pub next_reading_id: Option<String>,
}

/// Case-insensitive identity without Unicode canonical normalization. This
/// retains the existing distinction between precomposed and decomposed tags.
pub fn tag_key(tag: &str) -> String {
    tag.trim().case_fold().collect()
}

/// User tags lead and win spelling when a machine produces the same tag.
pub fn tag_entries(metadata: &Metadata) -> Vec<TagEntry> {
    tag_entries_with_current_sources(metadata, None, None, false)
}

/// Project only inference for the source bytes in this scanned reading.
pub fn tag_entries_for_reading(
    metadata: &Metadata,
    body: &str,
    visual_hash: Option<&str>,
) -> Vec<TagEntry> {
    let text_hash = text_source_content(metadata, body)
        .map(|(title, excerpt, content)| text_fingerprint(title, excerpt, content));
    tag_entries_with_current_sources(metadata, text_hash.as_deref(), visual_hash, true)
}

fn tag_entries_with_current_sources(
    metadata: &Metadata,
    text_hash: Option<&str>,
    visual_hash: Option<&str>,
    current_only: bool,
) -> Vec<TagEntry> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tag in &metadata.tags {
        let key = tag_key(tag);
        if !key.is_empty() && seen.insert(key.clone()) {
            entries.push(TagEntry {
                name: tag.clone(),
                key,
                origin: "user".into(),
            });
        }
    }
    let excluded: std::collections::HashSet<_> = metadata
        .excluded_machine_tags
        .iter()
        .map(|tag| tag_key(tag))
        .collect();
    for source in &metadata.machine_tags {
        if current_only
            && ((source.source == "text" && text_hash != Some(source.source_fingerprint.as_str()))
                || (source.source == "image"
                    && visual_hash != Some(source.source_fingerprint.as_str())))
        {
            continue;
        }
        for tag in &source.tags {
            let key = tag_key(tag);
            if !key.is_empty() && !excluded.contains(&key) && seen.insert(key.clone()) {
                entries.push(TagEntry {
                    name: tag.clone(),
                    key,
                    origin: "machine".into(),
                });
            }
        }
    }
    entries
}

pub fn effective_tags(metadata: &Metadata) -> Vec<String> {
    tag_entries(metadata)
        .into_iter()
        .map(|entry| entry.name)
        .collect()
}

pub fn active_machine_tags(metadata: &Metadata) -> Vec<String> {
    tag_entries(metadata)
        .into_iter()
        .filter(|entry| entry.origin == "machine")
        .map(|entry| entry.name)
        .collect()
}

fn normalize_machine_tags(tags: &[String], limit: usize) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tag in tags {
        let mut value = String::new();
        let mut separator = false;
        for character in tag.trim().to_lowercase().chars() {
            if character.is_whitespace() || character == '_' {
                separator = !value.is_empty();
            } else {
                if separator && !value.ends_with('-') {
                    value.push('-');
                }
                value.push(character);
                separator = false;
            }
        }
        let value = value.trim_matches('-').to_string();
        if !value.is_empty() && value.chars().count() <= MAX_TAG_LEN && seen.insert(tag_key(&value))
        {
            normalized.push(value);
            if normalized.len() == limit {
                break;
            }
        }
    }
    normalized
}

fn replace_machine_source(
    metadata: &mut Metadata,
    source: &str,
    fingerprint: &str,
    analyzer_version: &str,
    tags: &[String],
) -> bool {
    let replacement = MachineTagSource {
        source: source.into(),
        source_fingerprint: fingerprint.into(),
        analyzer_version: analyzer_version.into(),
        tags: normalize_machine_tags(tags, MAX_MACHINE_TAGS_PER_SOURCE),
    };
    if let Some(existing) = metadata
        .machine_tags
        .iter_mut()
        .find(|entry| entry.source == source)
    {
        if *existing == replacement {
            return false;
        }
        *existing = replacement;
    } else {
        metadata.machine_tags.push(replacement);
    }
    true
}

fn text_source_content<'a>(
    metadata: &'a Metadata,
    body_markdown: &'a str,
) -> Option<(&'a str, &'a str, &'a str)> {
    let title = metadata.title.trim();
    let excerpt = metadata.excerpt.as_deref().unwrap_or("").trim();
    let body = if metadata.lightweight || metadata.kind.is_media() {
        ""
    } else {
        body_markdown.trim()
    };
    let generic_title = title.is_empty()
        || title.eq_ignore_ascii_case("saved link")
        || title.eq_ignore_ascii_case("pasted image")
        || title.eq_ignore_ascii_case("imported image")
        || title.eq_ignore_ascii_case("imported video")
        || title == metadata.url;
    if generic_title && excerpt.is_empty() && body.is_empty() {
        return None;
    }
    Some((title, excerpt, body))
}

fn text_fingerprint(title: &str, excerpt: &str, body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(b"\n");
    hasher.update(excerpt.as_bytes());
    hasher.update(b"\n");
    hasher.update(body.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn text_tagging_input(metadata: &Metadata, body_markdown: &str) -> Option<(String, String)> {
    let (title, excerpt, body) = text_source_content(metadata, body_markdown)?;
    let fingerprint = text_fingerprint(title, excerpt, body);
    let heading = format!("{title}\n{excerpt}\n");
    let remaining = MAX_TEXT_TAGGING_CHARS.saturating_sub(heading.chars().count());
    let body_chars: Vec<char> = body.chars().collect();
    let sampled_body: String = if body_chars.len() <= remaining {
        body.into()
    } else if remaining < 30 {
        String::new()
    } else {
        let each_end = remaining / 3;
        let middle = remaining.saturating_sub(each_end * 2 + 12);
        let midpoint = body_chars.len() / 2;
        format!(
            "{}\n…\n{}\n…\n{}",
            body_chars[..each_end].iter().collect::<String>(),
            body_chars[midpoint.saturating_sub(middle / 2)..][..middle]
                .iter()
                .collect::<String>(),
            body_chars[body_chars.len() - each_end..]
                .iter()
                .collect::<String>()
        )
    };
    let text: String = format!("{heading}{sampled_body}")
        .chars()
        .take(MAX_TEXT_TAGGING_CHARS)
        .collect();
    Some((text, fingerprint))
}

/// Scan a bounded ID window; completed sources are skipped without relying on
/// disposable DB state. A missing model leaves the file untouched and pending.
pub fn pending_text_tagging(
    conn: &Connection,
    library: &LibraryRoot,
    analyzer_version: &str,
    limit: usize,
    after_reading_id: Option<&str>,
) -> Result<TextTaggingBatch> {
    let (ids, next_reading_id) =
        pending_text_tagging_ids(conn, analyzer_version, limit, after_reading_id)?;
    Ok(pending_text_tagging_for_ids(
        library,
        analyzer_version,
        ids,
        next_reading_id,
    ))
}

/// Fetch only the bounded index window while the database connection is held.
pub fn pending_text_tagging_ids(
    conn: &Connection,
    analyzer_version: &str,
    limit: usize,
    after_reading_id: Option<&str>,
) -> Result<(Vec<String>, Option<String>)> {
    if analyzer_version.trim().is_empty() {
        bail!("analyzer version must not be blank");
    }
    let limit = limit.clamp(1, 64);
    let mut stmt = conn.prepare("SELECT id FROM readings WHERE id > ?1 ORDER BY id LIMIT ?2")?;
    let ids = stmt
        .query_map(
            rusqlite::params![after_reading_id.unwrap_or(""), limit as i64],
            |row| row.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next_reading_id = (ids.len() == limit).then(|| ids.last().unwrap().clone());
    Ok((ids, next_reading_id))
}

/// Stage file-backed model input after releasing the database connection.
pub fn pending_text_tagging_for_ids(
    library: &LibraryRoot,
    analyzer_version: &str,
    ids: Vec<String>,
    next_reading_id: Option<String>,
) -> TextTaggingBatch {
    let mut tasks = Vec::new();
    for id in ids {
        let Ok(Some(reading)) = crate::scanner::read_reading_for_id(library, &id) else {
            continue;
        };
        let Some((text, fingerprint)) = text_tagging_input(&reading.metadata, &reading.body) else {
            continue;
        };
        if reading.metadata.machine_tags.iter().any(|source| {
            source.source == "text"
                && source.source_fingerprint == fingerprint
                && source.analyzer_version == analyzer_version
        }) {
            continue;
        }
        tasks.push(TextTaggingTask {
            reading_id: id,
            title: reading.metadata.title,
            text,
            source_fingerprint: fingerprint,
            analyzer_version: analyzer_version.into(),
        });
    }
    TextTaggingBatch {
        tasks,
        next_reading_id,
    }
}

/// Apply a text-model result only to the exact file content that was analyzed.
pub fn complete_text_tagging(
    library: &LibraryRoot,
    conn: &Connection,
    task: &TextTaggingTask,
    tags: &[String],
) -> Result<bool> {
    if task.analyzer_version.trim().is_empty() {
        bail!("analyzer version must not be blank");
    }
    let lock = lock_reading(library, &task.reading_id)?;
    let Some(mut reading) = crate::scanner::read_reading_for_id(library, &task.reading_id)? else {
        return Ok(false);
    };
    let Some((_, fingerprint)) = text_tagging_input(&reading.metadata, &reading.body) else {
        return Ok(false);
    };
    if fingerprint != task.source_fingerprint {
        return Ok(false);
    }
    if replace_machine_source(
        &mut reading.metadata,
        "text",
        &fingerprint,
        &task.analyzer_version,
        tags,
    ) {
        let written = write_reading_under_lock(library, reading.metadata, reading.body, &lock)?;
        sync_index(library, conn, &written.metadata.id)?;
    }
    Ok(true)
}

/// Persist a conservative subset of Vision's existing labels for a preview.
/// The hash check also rejects a source replaced by an external sync writer.
pub fn complete_image_tagging_file(
    library: &LibraryRoot,
    id: &str,
    content_hash: &str,
    analyzer_version: &str,
    labels: &[crate::VisualLabel],
) -> Result<Option<bool>> {
    if labels
        .iter()
        .any(|label| !label.confidence.is_finite() || !(0.0..=1.0).contains(&label.confidence))
    {
        bail!("image label confidences must be finite values from 0...1");
    }
    let lock = lock_reading(library, id)?;
    let Some(reading) = crate::scanner::read_reading_for_id(library, id)? else {
        return Ok(None);
    };
    let mut ordered = labels.to_vec();
    ordered.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
    let candidates: Vec<String> = ordered
        .iter()
        .filter(|label| label.confidence >= IMAGE_TAG_MIN_CONFIDENCE)
        .take(MAX_IMAGE_TAGS)
        .map(|label| label.identifier.clone())
        .collect();
    let normalized = normalize_machine_tags(&candidates, MAX_MACHINE_TAGS_PER_SOURCE);
    let Some(asset) = reading.metadata.preview_asset.as_deref() else {
        return Ok(None);
    };
    let Ok(inspected) = crate::visual_index::inspect_asset_for_tagging(library, id, asset) else {
        return Ok(None);
    };
    if inspected.content_hash != content_hash {
        return Ok(None);
    }
    // An external sync writer may update article.md while the asset is hashed.
    // Apply inference to the latest safe reading so manual tags, exclusions, and
    // body edits made during that I/O are retained.
    let Some(mut reading) = crate::scanner::read_reading_for_id(library, id)? else {
        return Ok(None);
    };
    if reading.metadata.preview_asset.as_deref() != Some(asset) {
        return Ok(None);
    }
    if reading.metadata.machine_tags.iter().any(|source| {
        source.source == "image"
            && source.source_fingerprint == content_hash
            && source.analyzer_version == analyzer_version
            && source.tags == normalized
    }) {
        return Ok(Some(false));
    }
    if replace_machine_source(
        &mut reading.metadata,
        "image",
        content_hash,
        analyzer_version,
        &candidates,
    ) {
        write_reading_under_lock(library, reading.metadata, reading.body, &lock)?;
        return Ok(Some(true));
    }
    Ok(Some(false))
}

/// Every reading whose current index projection references these pixels.
pub fn reading_ids_for_visual_hash(conn: &Connection, content_hash: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT id FROM readings WHERE visual_asset_hash=?1 ORDER BY id")?;
    let ids = stmt
        .query_map([content_hash], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    Ok(ids)
}

/// File-hydrate cached visual labels in the same bounded ID window whose DB
/// projections were hydrated. Unchanged source stamps avoid rewriting files.
pub fn cached_image_tagging_batch(
    conn: &Connection,
    analyzer_version: &str,
    after_reading_id: Option<&str>,
    limit: usize,
) -> Result<Vec<(String, String, Vec<crate::VisualLabel>)>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.visual_asset_hash, a.labels_json, a.supported
         FROM readings r LEFT JOIN visual_analysis a
           ON a.content_hash=r.visual_asset_hash AND a.analyzer_version=?1
         WHERE r.id > ?2 ORDER BY r.id LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![
                analyzer_version,
                after_reading_id.unwrap_or(""),
                limit as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i32>>(3)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut candidates = Vec::new();
    for (id, hash, labels_json, supported) in rows {
        let (Some(hash), Some(labels_json), Some(supported)) = (hash, labels_json, supported)
        else {
            continue;
        };
        let labels: Vec<crate::VisualLabel> = if supported != 0 {
            serde_json::from_str(&labels_json)?
        } else {
            Vec::new()
        };
        candidates.push((id, hash, labels));
    }
    Ok(candidates)
}

/// Validate and normalize an imported tag according to the library format.
///
/// Interactive tag entry retains its established behavior in [`add_tag`]. A
/// migration crosses a stricter trust boundary, so imported state must already
/// be lowercase and whitespace-free after surrounding whitespace is trimmed.
pub(crate) fn validate_imported_tag(tag: &str) -> Result<String> {
    let tag = tag.trim();
    if tag.is_empty() {
        bail!("tag must not be empty");
    }

    let len = tag.chars().count();
    if len > MAX_TAG_LEN {
        bail!("tag is too long: {len} characters (max {MAX_TAG_LEN})");
    }
    if tag.chars().any(char::is_whitespace) {
        bail!("tag must not contain whitespace: {tag:?}");
    }
    if tag.to_lowercase() != tag {
        bail!("tag must be lowercase: {tag:?}");
    }

    Ok(tag.to_string())
}

/// Add `tag` to the reading identified by `id`.
///
/// No-ops if the tag is already present. Rejects a tag longer than
/// [`MAX_TAG_LEN`] characters. Updates the `.md` file first, then syncs the
/// index row.
pub fn add_tag(library: &LibraryRoot, conn: &Connection, id: &str, tag: &str) -> Result<()> {
    let lock = lock_reading(library, id)?;
    let Some(mut reading) = crate::scanner::read_reading_for_id(library, id)? else {
        bail!("reading not found: {id}")
    };

    let tag = tag.trim().to_string();
    if tag.is_empty() {
        bail!("tag must not be empty");
    }
    let len = tag.chars().count();
    if len > MAX_TAG_LEN {
        bail!("tag is too long: {len} characters (max {MAX_TAG_LEN})");
    }
    let key = tag_key(&tag);
    let before_exclusions = reading.metadata.excluded_machine_tags.len();
    reading
        .metadata
        .excluded_machine_tags
        .retain(|excluded| tag_key(excluded) != key);
    if reading
        .metadata
        .tags
        .iter()
        .any(|existing| tag_key(existing) == key)
        && reading.metadata.excluded_machine_tags.len() == before_exclusions
    {
        return Ok(());
    }
    if !reading
        .metadata
        .tags
        .iter()
        .any(|existing| tag_key(existing) == key)
    {
        reading.metadata.tags.push(tag);
    }
    let written = write_reading_under_lock(library, reading.metadata, reading.body, &lock)?;
    sync_index(library, conn, &written.metadata.id)
}

/// Remove `tag` from the reading identified by `id`.
///
/// No-ops if the tag is not present. Updates the `.md` file first, then
/// syncs the index row.
pub fn remove_tag(library: &LibraryRoot, conn: &Connection, id: &str, tag: &str) -> Result<()> {
    let lock = lock_reading(library, id)?;
    let Some(mut reading) = crate::scanner::read_reading_for_id(library, id)? else {
        bail!("reading not found: {id}")
    };

    let key = tag_key(tag);
    let before = reading.metadata.tags.len();
    reading.metadata.tags.retain(|t| tag_key(t) != key);
    let suppressed = !reading
        .metadata
        .excluded_machine_tags
        .iter()
        .any(|excluded| tag_key(excluded) == key);
    if suppressed {
        reading.metadata.excluded_machine_tags.push(key);
    }
    if reading.metadata.tags.len() == before && !suppressed {
        return Ok(());
    }

    let written = write_reading_under_lock(library, reading.metadata, reading.body, &lock)?;
    sync_index(library, conn, &written.metadata.id)
}

/// Return the tags to show in the sidebar's Tags section, each with its badge
/// count, sorted **alphabetically by name**.
///
/// The tiles behave like the smart-view rows, which are a fixed set: *every* tag
/// in the library is always present, so switching view, searching, or picking a
/// rating only changes the *badge*, zeroing it rather than hiding the tile. This
/// is the Tags facet, so `scope`'s own tag selection is ignored (a facet never
/// filters itself).
///
/// The order is alphabetical, not by count, precisely *because* the set is fixed:
/// a count-based order would make tiles jump around as a search or facet changes
/// their badges. Alphabetical keeps every tile in the same position no matter
/// what the counts do.
///
/// The count re-applies the full view, the search, and the selected rating
/// through a `COUNT(...) FILTER`, so a tag with no matching reading reports 0
/// while keeping its tile. With the default scope (`view = All`) an archived-only
/// tag therefore shows a 0, and every other tag its plain non-archived count.
pub fn list_tags(conn: &Connection, scope: &CountScope) -> Result<Vec<(String, u64)>> {
    let search = ResolvedSearch::resolve_scoped(conn, scope)?;
    list_tags_with(conn, scope, &search)
}

/// [`list_tags`] with the search pre-resolved, so [`crate::list::sidebar_counts`]
/// can share one resolution across all three sidebar sections.
pub(crate) fn list_tags_with(
    conn: &Connection,
    scope: &CountScope,
    search: &ResolvedSearch,
) -> Result<Vec<(String, u64)>> {
    // Presence = every tag in the library (fixed set). The badge narrows to the
    // exact view plus the search and sibling rating facet.
    let mut vals: Vec<Value> = Vec::new();
    let count_filter = pinned_count_filter(scope, Facet::Rating, search, &mut vals);

    let sql = format!(
        "SELECT COALESCE(
             MIN(CASE WHEN json_extract(tag_entry.value, '$.origin') = 'user'
                      THEN json_extract(tag_entry.value, '$.name') END),
             MIN(json_extract(tag_entry.value, '$.name'))
         ) AS display_name,
         COUNT(*) FILTER (WHERE {count_filter}) AS cnt
         FROM readings, json_each(readings.tag_entries_json) tag_entry
         GROUP BY json_extract(tag_entry.value, '$.key')
         ORDER BY json_extract(tag_entry.value, '$.key') ASC"
    );
    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(params_from_iter(vals), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
    })?;

    rows.map(|r| r.map_err(Into::into)).collect()
}

/// Re-read the article file from disk and update its index row.
fn sync_index(library: &LibraryRoot, conn: &Connection, id: &str) -> Result<()> {
    apply_diffs(conn, &[ScanDiff::Changed(scan_for_index(library, id)?)])
}

pub(crate) fn scan_for_index(library: &LibraryRoot, id: &str) -> Result<ScannedReading> {
    let path = library.article_path(id);
    let Some((reading, modified_at)) =
        crate::scanner::read_reading_with_modified_at_for_id(library, id)?
    else {
        bail!("reading not found: {id}")
    };
    let visual_asset = reading.metadata.preview_asset.as_deref().and_then(|asset| {
        if matches!(
            reading.metadata.kind,
            crate::ReadingKind::Article | crate::ReadingKind::Image
        ) {
            crate::visual_index::inspect_image_asset_for_tagging(
                library,
                &reading.metadata.id,
                asset,
            )
            .ok()
        } else {
            crate::visual_index::inspect_asset_for_tagging(library, &reading.metadata.id, asset)
                .ok()
        }
    });
    let media_aspect_ratio = crate::scanner::inspect_media_aspect_ratio(
        library,
        &reading.metadata,
        visual_asset.as_ref(),
    );

    Ok(ScannedReading {
        id: reading.metadata.id.clone(),
        source_hash: reading.metadata.source_hash.clone(),
        modified_at,
        path,
        has_note: crate::scanner::note_file_exists(library, &reading.metadata.id),
        visual_asset,
        media_aspect_ratio,
        body: reading.body,
        metadata: reading.metadata,
    })
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

    fn meta(id: &str, url: &str) -> Metadata {
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

    #[test]
    fn add_tag_updates_frontmatter() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        write_reading(&lib, meta(&id, "https://example.com"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        add_tag(&lib, &conn, &id, "rust").unwrap();

        let content = std::fs::read_to_string(lib.article_path(&id)).unwrap();
        assert!(content.contains("rust"), "tag should appear in frontmatter");
    }

    #[test]
    fn add_tag_updates_index() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        write_reading(&lib, meta(&id, "https://example.com"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();
        add_tag(&lib, &conn, &id, "rust").unwrap();

        let tags = list_tags(&conn, &CountScope::default()).unwrap();
        assert_eq!(tags, vec![("rust".to_string(), 1)]);
    }

    #[test]
    fn add_tag_is_idempotent() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        write_reading(&lib, meta(&id, "https://example.com"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        add_tag(&lib, &conn, &id, "rust").unwrap();
        add_tag(&lib, &conn, &id, "rust").unwrap();

        let tags = list_tags(&conn, &CountScope::default()).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].1, 1);
    }

    #[test]
    fn add_tag_accepts_tag_at_max_length() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        write_reading(&lib, meta(&id, "https://example.com"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // Exactly MAX_TAG_LEN characters is allowed (the boundary is inclusive).
        let tag = "a".repeat(MAX_TAG_LEN);
        add_tag(&lib, &conn, &id, &tag).unwrap();

        let tags = list_tags(&conn, &CountScope::default()).unwrap();
        assert_eq!(tags, vec![(tag, 1)]);
    }

    #[test]
    fn add_tag_rejects_tag_over_max_length() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        write_reading(&lib, meta(&id, "https://example.com"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // One past the limit is rejected, and nothing is written to the index.
        let tag = "a".repeat(MAX_TAG_LEN + 1);
        assert!(add_tag(&lib, &conn, &id, &tag).is_err());
        assert!(list_tags(&conn, &CountScope::default()).unwrap().is_empty());
    }

    #[test]
    fn add_tag_counts_length_in_characters_not_bytes() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        write_reading(&lib, meta(&id, "https://example.com"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // 25 multi-byte characters (é is 2 bytes) is 50 bytes but only 25 chars,
        // so it must be accepted — the limit counts characters, not bytes.
        let tag = "é".repeat(MAX_TAG_LEN);
        add_tag(&lib, &conn, &id, &tag).unwrap();

        let tags = list_tags(&conn, &CountScope::default()).unwrap();
        assert_eq!(tags, vec![(tag, 1)]);
    }

    #[test]
    fn add_tag_measures_length_after_trimming() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        write_reading(&lib, meta(&id, "https://example.com"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // Surrounding whitespace is trimmed before the length check, so a
        // MAX_TAG_LEN name padded with spaces still fits.
        let padded = format!("   {}   ", "a".repeat(MAX_TAG_LEN));
        add_tag(&lib, &conn, &id, &padded).unwrap();

        let tags = list_tags(&conn, &CountScope::default()).unwrap();
        assert_eq!(tags, vec![("a".repeat(MAX_TAG_LEN), 1)]);
    }

    #[test]
    fn imported_tag_validation_enforces_the_library_format() {
        assert_eq!(
            validate_imported_tag("  local-first  ").unwrap(),
            "local-first"
        );
        assert!(validate_imported_tag("").is_err());
        assert!(validate_imported_tag("   ").is_err());
        assert!(validate_imported_tag("Local-first").is_err());
        assert!(validate_imported_tag("local first").is_err());
        assert!(validate_imported_tag("local\tfirst").is_err());
        assert!(validate_imported_tag(&"a".repeat(MAX_TAG_LEN + 1)).is_err());
        assert!(validate_imported_tag(&"é".repeat(MAX_TAG_LEN)).is_ok());
    }

    #[test]
    fn remove_tag_updates_frontmatter_and_index() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        let mut m = meta(&id, "https://example.com");
        m.tags = vec!["rust".into(), "async".into()];
        write_reading(&lib, m, "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        remove_tag(&lib, &conn, &id, "rust").unwrap();

        let content = std::fs::read_to_string(lib.article_path(&id)).unwrap();
        let metadata = parse_reading(&content).unwrap().metadata;
        assert_eq!(metadata.tags, vec!["async"]);
        assert_eq!(metadata.excluded_machine_tags, vec!["rust"]);

        let tags = list_tags(&conn, &CountScope::default()).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].0, "async");
    }

    #[test]
    fn remove_tag_noop_when_absent() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();

        write_reading(&lib, meta(&id, "https://example.com"), "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // Should not error even though the tag doesn't exist.
        remove_tag(&lib, &conn, &id, "nonexistent").unwrap();
    }

    #[test]
    fn list_tags_counts_across_readings() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let id1 = new_id();
        let id2 = new_id();
        let mut m1 = meta(&id1, "https://a.com");
        m1.tags = vec!["rust".into(), "async".into()];
        let mut m2 = meta(&id2, "https://b.com");
        m2.tags = vec!["rust".into()];

        write_reading(&lib, m1, "body".into()).unwrap();
        write_reading(&lib, m2, "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let tags = list_tags(&conn, &CountScope::default()).unwrap();
        // Sorted alphabetically by name (not by count): async before rust.
        assert_eq!(tags[0], ("async".to_string(), 1));
        assert_eq!(tags[1], ("rust".to_string(), 2));
    }

    #[test]
    fn list_tags_includes_legacy_archived_readings_in_all() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut m = meta(&new_id(), "https://a.com");
        m.tags = vec!["hidden".into()];
        m.archived = true;
        write_reading(&lib, m, "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // All is the complete inspiration library, so cards written by older builds
        // remain discoverable even when their metadata still says archived.
        let tags = list_tags(&conn, &CountScope::default()).unwrap();
        assert_eq!(tags, vec![("hidden".to_string(), 1)]);
    }

    /// Build the faceting corpus shared by the scoped tag-count tests:
    /// A (active, rust+prog, 5★, "alpha"), B (active, rust, 3★, "beta"),
    /// C (archived, rust+old, 5★, "gamma").
    fn faceting_corpus(lib: &LibraryRoot, conn: &Connection) {
        let mut a = meta(&new_id(), "https://a.com");
        a.tags = vec!["rust".into(), "prog".into()];
        a.rating = 5;
        write_reading(lib, a, "alpha".into()).unwrap();
        let mut b = meta(&new_id(), "https://b.com");
        b.tags = vec!["rust".into()];
        b.rating = 3;
        write_reading(lib, b, "beta".into()).unwrap();
        let mut c = meta(&new_id(), "https://c.com");
        c.tags = vec!["rust".into(), "old".into()];
        c.rating = 5;
        c.archived = true;
        write_reading(lib, c, "gamma".into()).unwrap();
        rebuild(conn, lib).unwrap();
    }

    #[test]
    fn list_tags_scoped_by_search() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // "beta" is only in B (tags: rust). The search scopes the *counts*, not the
        // visible tiles: rust reports 1, while "prog" (on A, which doesn't match)
        // and "old" (archived) both stay pinned at 0 rather than disappearing.
        // Order is alphabetical, independent of the counts.
        let tags = list_tags(
            &conn,
            &CountScope {
                query: Some("beta".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tags,
            vec![
                ("old".to_string(), 0),
                ("prog".to_string(), 0),
                ("rust".to_string(), 1),
            ]
        );
    }

    #[test]
    fn list_tags_follow_the_archive_view_facet() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // Selecting the Archive view flips the tag counts to the archived side:
        // C's tags (rust, old) count, while the active-only "prog" stays pinned at 0.
        let tags = list_tags(
            &conn,
            &CountScope {
                view: crate::list::View::Archive,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tags,
            vec![
                ("old".to_string(), 1),
                ("prog".to_string(), 0),
                ("rust".to_string(), 1),
            ]
        );
    }

    #[test]
    fn list_tags_scoped_by_rating_facet() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // The legacy 5★ facet still sees both A and archived C in the complete All
        // library, so rust reports 2 while prog and old each report 1.
        let tags = list_tags(
            &conn,
            &CountScope {
                rating: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tags,
            vec![
                ("old".to_string(), 1),
                ("prog".to_string(), 1),
                ("rust".to_string(), 2),
            ]
        );
    }

    #[test]
    fn list_tags_pins_zero_count_tiles_under_a_rating_facet() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        // A: rust, 5★. B: cook, 3★. Both non-archived.
        let mut a = meta(&new_id(), "https://a.com");
        a.tags = vec!["rust".into()];
        a.rating = 5;
        write_reading(&lib, a, "alpha".into()).unwrap();
        let mut b = meta(&new_id(), "https://b.com");
        b.tags = vec!["cook".into()];
        b.rating = 3;
        write_reading(&lib, b, "beta".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // Selecting 5★ keeps BOTH tiles visible (presence ignores the rating
        // facet); "cook" reports 0 instead of vanishing, since its only reading
        // is 3★. Order is alphabetical: cook before rust.
        let tags = list_tags(
            &conn,
            &CountScope {
                rating: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(tags, vec![("cook".to_string(), 0), ("rust".to_string(), 1)]);
    }

    #[test]
    fn list_tags_pins_zero_under_search_plus_rating_facet() {
        // The exact sidebar flow: search narrows the set, then a rating facet is
        // clicked. Tags matching the search stay pinned; those with no reading at
        // the selected rating show 0 instead of disappearing.
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut a = meta(&new_id(), "https://a.com"); // scifi, 5★, matches search
        a.tags = vec!["scifi".into()];
        a.rating = 5;
        write_reading(&lib, a, "a lone starship".into()).unwrap();
        let mut b = meta(&new_id(), "https://b.com"); // space, 3★, matches search
        b.tags = vec!["space".into()];
        b.rating = 3;
        write_reading(&lib, b, "a docking starship".into()).unwrap();
        let mut c = meta(&new_id(), "https://c.com"); // cooking, 5★, NO search match
        c.tags = vec!["cooking".into()];
        c.rating = 5;
        write_reading(&lib, c, "today I cooked pasta".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let tags = list_tags(
            &conn,
            &CountScope {
                query: Some("starship".into()),
                rating: Some(5),
                ..Default::default()
            },
        )
        .unwrap();
        // Presence is the whole (non-archived) view, so every tag stays visible:
        // scifi counts its one 5★ starship hit; space (3★) and cooking (no search
        // match at all) both stay pinned at 0. Order is alphabetical.
        assert_eq!(
            tags,
            vec![
                ("cooking".to_string(), 0),
                ("scifi".to_string(), 1),
                ("space".to_string(), 0),
            ]
        );
    }

    #[test]
    fn list_tags_pins_all_under_search_plus_tag_selection() {
        // The reported case: a tag is selected AND a search is typed. Every tag in
        // the view must stay visible; only the badges reflect the search.
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut a = meta(&new_id(), "https://a.com"); // scifi, matches "starship"
        a.tags = vec!["scifi".into()];
        a.rating = 5;
        write_reading(&lib, a, "a lone starship".into()).unwrap();
        let mut b = meta(&new_id(), "https://b.com"); // space, matches "starship"
        b.tags = vec!["space".into()];
        b.rating = 3;
        write_reading(&lib, b, "a docking starship".into()).unwrap();
        let mut c = meta(&new_id(), "https://c.com"); // cooking, NO "starship"
        c.tags = vec!["cooking".into()];
        c.rating = 2;
        write_reading(&lib, c, "today I cooked pasta".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let tags = list_tags(
            &conn,
            &CountScope {
                tag: Some("scifi".into()),
                query: Some("starship".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // The selected tag is the Tags section's own axis, so it's ignored here —
        // every tag stays. "cooking" (no search match) is pinned at 0. Order is
        // alphabetical.
        assert_eq!(
            tags,
            vec![
                ("cooking".to_string(), 0),
                ("scifi".to_string(), 1),
                ("space".to_string(), 1),
            ]
        );
    }

    #[test]
    fn list_tags_pins_across_the_unread_view() {
        // The reported case: searching, then clicking a smart view (Unread) must
        // not drop tags. A tag whose only reading is *read* stays visible at 0,
        // because All/Unread/Read share one presence pool.
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut a = meta(&new_id(), "https://a.com"); // unread, #alpha
        a.tags = vec!["alpha".into()];
        write_reading(&lib, a, "coding in rust".into()).unwrap();
        let mut b = meta(&new_id(), "https://b.com"); // read, #beta
        b.tags = vec!["beta".into()];
        b.read_at = Some("2026-06-13T16:00:00.000Z".into());
        write_reading(&lib, b, "coding in swift".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let tags = list_tags(
            &conn,
            &CountScope {
                view: crate::list::View::Unread,
                query: Some("coding".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // "beta" (on the read article) is pinned at 0 rather than hidden.
        assert_eq!(
            tags,
            vec![("alpha".to_string(), 1), ("beta".to_string(), 0)]
        );
    }

    #[test]
    fn list_tags_pins_across_the_favorites_view_without_search() {
        // The reported case: selecting Favorites (no search) must keep every tag.
        // A tag on a non-favorite reading stays visible at 0.
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut a = meta(&new_id(), "https://a.com"); // favorite, #keep
        a.tags = vec!["keep".into()];
        a.favorite = true;
        write_reading(&lib, a, "body".into()).unwrap();
        let mut b = meta(&new_id(), "https://b.com"); // not favorite, #other
        b.tags = vec!["other".into()];
        write_reading(&lib, b, "body".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        let tags = list_tags(
            &conn,
            &CountScope {
                view: crate::list::View::Favorites,
                ..Default::default()
            },
        )
        .unwrap();
        // "other" (on the non-favorite reading) is pinned at 0, not hidden.
        assert_eq!(
            tags,
            vec![("keep".to_string(), 1), ("other".to_string(), 0)]
        );
    }

    #[test]
    fn list_tags_order_is_alphabetical_and_stable_across_search() {
        // Tiles must keep the same position regardless of counts, so a search
        // never reshuffles them. zulu has the highest count, alpha the lowest, yet
        // the order stays alphabetical both unfiltered and under a search.
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let mut a = meta(&new_id(), "https://a.com"); // alpha, matches "widget"
        a.tags = vec!["alpha".into()];
        write_reading(&lib, a, "a widget".into()).unwrap();
        let mut z1 = meta(&new_id(), "https://z1.com"); // zulu x2, no "widget"
        z1.tags = vec!["zulu".into()];
        write_reading(&lib, z1, "nothing here".into()).unwrap();
        let mut z2 = meta(&new_id(), "https://z2.com");
        z2.tags = vec!["zulu".into()];
        write_reading(&lib, z2, "still nothing".into()).unwrap();
        rebuild(&conn, &lib).unwrap();

        // Unfiltered: alpha(1) before zulu(2) despite zulu's higher count.
        let names = |tags: Vec<(String, u64)>| tags.into_iter().map(|t| t.0).collect::<Vec<_>>();
        assert_eq!(
            names(list_tags(&conn, &CountScope::default()).unwrap()),
            vec!["alpha", "zulu"]
        );

        // Under a search that zeroes zulu, the positions are unchanged.
        let searched = list_tags(
            &conn,
            &CountScope {
                query: Some("widget".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            searched,
            vec![("alpha".to_string(), 1), ("zulu".to_string(), 0)]
        );
    }

    #[test]
    fn list_tags_ignores_its_own_tag_selection() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        faceting_corpus(&lib, &conn);

        // A selected tag must NOT filter the tag list — the facet shows every
        // sibling tag so the user can switch — so it matches the default listing.
        let selected = list_tags(
            &conn,
            &CountScope {
                tag: Some("rust".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(selected, list_tags(&conn, &CountScope::default()).unwrap());
        assert_eq!(
            selected,
            vec![
                ("old".to_string(), 1),
                ("prog".to_string(), 1),
                ("rust".to_string(), 3),
            ]
        );
    }

    #[test]
    fn add_tag_returns_error_for_unknown_id() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);

        let result = add_tag(&lib, &conn, "nonexistent-id", "rust");
        assert!(result.is_err());
    }

    #[test]
    fn inferred_tags_merge_with_user_spelling_and_exact_filters_ignore_case() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let first = new_id();
        let second = new_id();
        let mut first_meta = meta(&first, "https://first.example");
        first_meta.tags = vec!["Chair".into()];
        write_reading(&lib, first_meta, "A wooden chair".into()).unwrap();
        write_reading(
            &lib,
            meta(&second, "https://second.example"),
            "An armchair in a room".into(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();

        let tasks = pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks;
        for task in tasks {
            complete_text_tagging(&lib, &conn, &task, &["chair".into(), "FURNITURE".into()])
                .unwrap();
        }

        assert_eq!(
            list_tags(&conn, &CountScope::default()).unwrap(),
            vec![("Chair".into(), 2), ("furniture".into(), 2)]
        );
        let rows = crate::list::list_readings(
            &conn,
            &crate::list::ListOptions {
                tag: Some("CHAIR".into()),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        let first_row = rows.iter().find(|row| row.id == first).unwrap();
        assert_eq!(first_row.tags, vec!["Chair", "furniture"]);
        assert_eq!(first_row.machine_tags, vec!["furniture"]);
        let second_row = rows.iter().find(|row| row.id == second).unwrap();
        assert_eq!(second_row.machine_tags, vec!["chair", "furniture"]);
    }

    #[test]
    fn unicode_full_case_folding_merges_facets_and_exact_filters() {
        assert_eq!(tag_key("Straße"), tag_key("STRASSE"));
        assert_eq!(tag_key("ΟΣ"), tag_key("ος"));
        assert_ne!(tag_key("Café"), tag_key("Cafe\u{301}"));
        assert_eq!(
            normalize_machine_tags(&["Straße".into(), "STRASSE".into()], 8),
            vec!["straße"]
        );

        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let mut first = meta(&new_id(), "https://first.example");
        first.tags = vec!["Straße".into()];
        write_reading(&lib, first, "First".into()).unwrap();
        let mut second = meta(&new_id(), "https://second.example");
        second.tags = vec!["STRASSE".into()];
        write_reading(&lib, second, "Second".into()).unwrap();
        rebuild(&conn, &lib).unwrap();
        let facets = list_tags(&conn, &CountScope::default()).unwrap();
        assert_eq!(facets.len(), 1);
        assert_eq!(facets[0].1, 2);
        let rows = crate::list::list_readings(
            &conn,
            &crate::list::ListOptions {
                tag: Some("strasse".into()),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn removed_inference_stays_suppressed_and_manual_add_restores_it() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com"),
            "A science article".into(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();
        let task = pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        complete_text_tagging(&lib, &conn, &task, &["Science".into()]).unwrap();
        assert_eq!(
            crate::list::get_reading(&conn, &id)
                .unwrap()
                .unwrap()
                .0
                .machine_tags,
            vec!["science"]
        );

        remove_tag(&lib, &conn, &id, "SCIENCE").unwrap();
        let task = pending_text_tagging(&conn, &lib, "text-v2", 64, None)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        complete_text_tagging(&lib, &conn, &task, &["science".into()]).unwrap();
        assert!(crate::list::get_reading(&conn, &id)
            .unwrap()
            .unwrap()
            .0
            .tags
            .is_empty());
        assert_eq!(
            parse_reading(&fs::read_to_string(lib.article_path(&id)).unwrap())
                .unwrap()
                .metadata
                .excluded_machine_tags,
            vec!["science"]
        );

        add_tag(&lib, &conn, &id, "Science").unwrap();
        let row = crate::list::get_reading(&conn, &id).unwrap().unwrap().0;
        assert_eq!(row.tags, vec!["Science"]);
        assert!(row.machine_tags.is_empty());
        let metadata = parse_reading(&fs::read_to_string(lib.article_path(&id)).unwrap())
            .unwrap()
            .metadata;
        assert!(metadata.excluded_machine_tags.is_empty());
    }

    #[test]
    fn changed_text_invalidates_inference_and_rejects_stale_completion() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(
            &lib,
            meta(&id, "https://example.com"),
            "Original subject".into(),
        )
        .unwrap();
        rebuild(&conn, &lib).unwrap();
        let original = pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        let metadata = parse_reading(&fs::read_to_string(lib.article_path(&id)).unwrap())
            .unwrap()
            .metadata;
        write_reading(&lib, metadata, "Revised subject".into()).unwrap();
        rebuild(&conn, &lib).unwrap();
        assert!(!complete_text_tagging(&lib, &conn, &original, &["old".into()]).unwrap());
        let revised = pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        assert_ne!(original.source_fingerprint, revised.source_fingerprint);
        complete_text_tagging(&lib, &conn, &revised, &["new".into()]).unwrap();
        assert_eq!(
            crate::list::get_reading(&conn, &id)
                .unwrap()
                .unwrap()
                .0
                .tags,
            vec!["new"]
        );

        let metadata = parse_reading(&fs::read_to_string(lib.article_path(&id)).unwrap())
            .unwrap()
            .metadata;
        write_reading(&lib, metadata, "Another subject".into()).unwrap();
        rebuild(&conn, &lib).unwrap();
        assert!(crate::list::get_reading(&conn, &id)
            .unwrap()
            .unwrap()
            .0
            .tags
            .is_empty());
    }

    #[test]
    fn text_task_is_bounded_and_samples_the_end() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        let body = format!("{} useful ending", "beginning ".repeat(1200));
        write_reading(&lib, meta(&id, "https://example.com"), body).unwrap();
        rebuild(&conn, &lib).unwrap();
        let task = pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        assert!(task.text.chars().count() <= MAX_TEXT_TAGGING_CHARS);
        assert!(task.text.contains("beginning"));
        assert!(task.text.contains("useful ending"));
    }

    #[test]
    fn text_tagging_rejects_swapped_article_symlink() {
        use std::os::unix::fs::symlink;

        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(&lib, meta(&id, "https://example.com"), "Original".into()).unwrap();
        rebuild(&conn, &lib).unwrap();
        let task = pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        let article = lib.article_path(&id);
        let outside = dir.path().join("outside.md");
        let original = fs::read_to_string(&article).unwrap();
        fs::write(&outside, &original).unwrap();
        fs::remove_file(&article).unwrap();
        symlink(&outside, &article).unwrap();

        assert!(pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks
            .is_empty());
        assert!(!complete_text_tagging(&lib, &conn, &task, &["private".into()]).unwrap());
        assert_eq!(fs::read_to_string(&outside).unwrap(), original);
    }

    #[test]
    fn text_tagging_rejects_mismatched_frontmatter_identity() {
        let (dir, conn) = setup();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(&lib, meta(&id, "https://example.com"), "Original".into()).unwrap();
        rebuild(&conn, &lib).unwrap();
        let task = pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        let article = lib.article_path(&id);
        let mut reading = parse_reading(&fs::read_to_string(&article).unwrap()).unwrap();
        reading.metadata.id = new_id();
        let foreign = crate::render_reading(&reading).unwrap();
        fs::write(&article, &foreign).unwrap();

        assert!(pending_text_tagging(&conn, &lib, "text-v1", 64, None)
            .unwrap()
            .tasks
            .is_empty());
        assert!(!complete_text_tagging(&lib, &conn, &task, &["wrong".into()]).unwrap());
        assert_eq!(fs::read_to_string(&article).unwrap(), foreign);
    }
}
