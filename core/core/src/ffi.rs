// SPDX-License-Identifier: MIT

//! UniFFI-exported interface for the macOS SwiftUI client.
//!
//! `Database` is the central object: open it with a SQLite path, then call
//! `rebuild` on first launch and `sync` on subsequent launches (or whenever
//! the library folder changes). Library mutations still take an explicit
//! `library_path`; the object additionally owns a disposable, per-device visual
//! snapshot cache derived beside `db_path` for safe platform image decoding.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use crate::{
    list::{CountScope, ListOptions, SortField, View},
    scanner::ScannedReading,
    LibraryRoot, ReadingKind, SaveDisposition, SaveOutcome,
};

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum CoreError {
    #[error("{0}")]
    Error(String),
}

fn e(err: impl std::fmt::Display) -> CoreError {
    CoreError::Error(err.to_string())
}

// ── Record types (plain data, no methods) ────────────────────────────────────

#[derive(uniffi::Record)]
pub struct FfiReadingRow {
    pub id: String,
    pub title: String,
    pub kind: FfiReadingKind,
    pub lightweight: bool,
    pub is_link: bool,
    pub has_note: bool,
    pub url: String,
    pub media_url: Option<String>,
    pub preview_asset: Option<String>,
    pub favicon_asset: Option<String>,
    pub theme_color: Option<String>,
    pub source_profile_json: Option<String>,
    pub dominant_color: Option<FfiWeightedColor>,
    pub media_aspect_ratio: Option<f64>,
    pub canonical_url: String,
    pub author: Option<String>,
    pub site: Option<String>,
    pub saved_at: String,
    pub read: bool,
    pub archived: bool,
    pub favorite: bool,
    pub rating: u8,
    pub excerpt: Option<String>,
    pub card_description: Option<String>,
    pub word_count: Option<u32>,
    pub lang: Option<String>,
    pub tags: Vec<String>,
}

#[derive(uniffi::Record)]
pub struct FfiTagCount {
    pub tag: String,
    pub count: u64,
}

#[derive(uniffi::Record)]
pub struct FfiRatingCount {
    pub rating: u8,
    pub count: u64,
}

#[derive(uniffi::Record)]
pub struct FfiViewCounts {
    pub all: u64,
    pub unread: u64,
    pub read: u64,
    pub archive: u64,
    pub favorites: u64,
}

/// Every sidebar count section in one payload: the view badges, the tag counts,
/// and the rating counts. Returned by [`Database::sidebar_counts`].
#[derive(uniffi::Record)]
pub struct FfiSidebarCounts {
    pub views: FfiViewCounts,
    pub tags: Vec<FfiTagCount>,
    pub ratings: Vec<FfiRatingCount>,
}

#[derive(uniffi::Record)]
pub struct FfiHighlight {
    pub id: String,
    pub text: String,
}

/// Result of importing content through the native app. Exact duplicates are a
/// successful, structured outcome so Swift can show friendly feedback without
/// parsing an error string.
#[derive(uniffi::Record)]
pub struct FfiImportResult {
    pub disposition: FfiImportDisposition,
    pub id: String,
    pub path: String,
}

/// One Inbox item retained for the user to inspect or retry.
#[derive(uniffi::Record)]
pub struct FfiInboxIssue {
    pub name: String,
    pub message: String,
}

/// A batch result, independent of the disposable index. The caller reconciles
/// once after processing the Inbox, not once for every imported reading.
#[derive(uniffi::Record)]
pub struct FfiInboxReport {
    pub saved: u32,
    pub duplicates: u32,
    pub pending: u32,
    pub issues: Vec<FfiInboxIssue>,
}

#[derive(uniffi::Record)]
pub struct FfiVisualAsset {
    pub reading_id: String,
    pub title: String,
    /// Reading-relative public path (`assets/<file>`).
    pub relative_path: String,
    /// App-owned, read-only staged path for platform image APIs. Never synced.
    pub absolute_file_path: String,
    pub content_hash: String,
}

#[derive(uniffi::Record)]
pub struct FfiVisualAnalysisTask {
    pub reading_id: String,
    pub relative_path: String,
    pub absolute_file_path: String,
    pub content_hash: String,
    pub analyzer_version: String,
}

#[derive(uniffi::Record)]
pub struct FfiPendingVisualAnalysis {
    pub tasks: Vec<FfiVisualAnalysisTask>,
    pub hydrated_count: u32,
}

#[derive(uniffi::Record)]
pub struct FfiVisualAssetsBatch {
    pub assets: Vec<FfiVisualAsset>,
    pub next_reading_id: Option<String>,
}

#[derive(uniffi::Record)]
pub struct FfiVisualAnalysisBatch {
    pub tasks: Vec<FfiVisualAnalysisTask>,
    pub hydrated_count: u32,
    pub next_reading_id: Option<String>,
}

#[derive(uniffi::Record)]
pub struct FfiVisualLabel {
    pub identifier: String,
    pub confidence: f64,
}

#[derive(uniffi::Record)]
pub struct FfiWeightedColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub weight: f64,
}

#[derive(uniffi::Record)]
pub struct FfiVisualAnalysisResult {
    pub supported: bool,
    pub labels: Vec<FfiVisualLabel>,
    pub palette: Vec<FfiWeightedColor>,
}

#[derive(uniffi::Enum)]
pub enum FfiImportDisposition {
    Saved,
    Upgraded,
    Duplicate,
}

#[derive(uniffi::Enum)]
pub enum FfiReadingKind {
    Article,
    Image,
    Video,
    Quote,
}

#[derive(uniffi::Enum)]
pub enum FfiView {
    All,
    Unread,
    Read,
    Archive,
    Favorites,
    Media,
    Articles,
    Notes,
    Links,
    Quotes,
}

#[derive(uniffi::Enum)]
pub enum FfiSortField {
    SavedAt,
    ReadAt,
    Rating,
    WordCount,
    Relevance,
}

#[derive(uniffi::Enum)]
pub enum FfiPredominantColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Brown,
    Black,
    Gray,
    White,
}

#[derive(uniffi::Record)]
pub struct FfiListOptions {
    pub view: FfiView,
    pub sort: FfiSortField,
    /// Ascending when `true`, descending when `false` (the default direction).
    pub ascending: bool,
    pub tag: Option<String>,
    pub rating: Option<u8>,
    pub kind: Option<FfiReadingKind>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub query: Option<String>,
    /// Exact tags that must all occur on the same reading.
    pub tag_terms: Vec<String>,
    /// Completed terms that must all occur in current supported visual analysis.
    pub visual_terms: Vec<String>,
    /// Palette colors selected as search tokens, encoded as #RRGGBB.
    pub color_terms: Vec<String>,
    pub item_type_terms: Vec<String>,
    pub predominant_color: Option<FfiPredominantColor>,
    /// Core Spotlight identifiers, ordered best-first for the same query.
    pub semantic_candidate_ids: Vec<String>,
    /// Core Spotlight identifiers matched from structured visual terms.
    pub visual_semantic_candidate_ids: Vec<String>,
    pub limit: u32,
    pub offset: u32,
}

/// The active board filters behind the faceted counts. Facets and every
/// free-text or structured search term compose as an intersection. Each count
/// query ignores its own facet axis while search terms and kind always compose
/// — see [`crate::list::CountScope`].
#[derive(uniffi::Record)]
pub struct FfiCountScope {
    pub view: FfiView,
    pub tag: Option<String>,
    pub rating: Option<u8>,
    pub kind: Option<FfiReadingKind>,
    pub query: Option<String>,
    pub tag_terms: Vec<String>,
    pub visual_terms: Vec<String>,
    pub predominant_color: Option<FfiPredominantColor>,
    pub semantic_candidate_ids: Vec<String>,
    pub visual_semantic_candidate_ids: Vec<String>,
}

// ── Conversions ──────────────────────────────────────────────────────────────

impl From<crate::list::ReadingRow> for FfiReadingRow {
    fn from(r: crate::list::ReadingRow) -> Self {
        Self {
            id: r.id,
            title: r.title,
            kind: r.kind.into(),
            lightweight: r.lightweight,
            is_link: r.is_link,
            has_note: r.has_note,
            url: r.url,
            media_url: r.media_url,
            preview_asset: r.preview_asset,
            favicon_asset: r.favicon_asset,
            theme_color: r.theme_color,
            source_profile_json: r.source_profile_json,
            dominant_color: r.dominant_color.map(|color| FfiWeightedColor {
                red: color.red,
                green: color.green,
                blue: color.blue,
                weight: color.weight,
            }),
            media_aspect_ratio: r.media_aspect_ratio,
            canonical_url: r.canonical_url,
            author: r.author,
            site: r.site,
            saved_at: r.saved_at,
            read: r.read,
            archived: r.archived,
            favorite: r.favorite,
            rating: r.rating,
            excerpt: r.excerpt,
            card_description: r.card_description,
            word_count: r.word_count,
            lang: r.lang,
            tags: r.tags,
        }
    }
}

impl From<ReadingKind> for FfiReadingKind {
    fn from(kind: ReadingKind) -> Self {
        match kind {
            ReadingKind::Article => Self::Article,
            ReadingKind::Image => Self::Image,
            ReadingKind::Video => Self::Video,
            ReadingKind::Quote => Self::Quote,
        }
    }
}

impl From<FfiReadingKind> for ReadingKind {
    fn from(kind: FfiReadingKind) -> Self {
        match kind {
            FfiReadingKind::Article => Self::Article,
            FfiReadingKind::Image => Self::Image,
            FfiReadingKind::Video => Self::Video,
            FfiReadingKind::Quote => Self::Quote,
        }
    }
}

impl From<crate::highlights::Highlight> for FfiHighlight {
    fn from(h: crate::highlights::Highlight) -> Self {
        Self {
            id: h.id,
            text: h.text,
        }
    }
}

impl From<SaveOutcome> for FfiImportResult {
    fn from(outcome: SaveOutcome) -> Self {
        Self {
            disposition: match outcome.disposition {
                SaveDisposition::Saved => FfiImportDisposition::Saved,
                SaveDisposition::Upgraded => FfiImportDisposition::Upgraded,
                SaveDisposition::Duplicate => FfiImportDisposition::Duplicate,
            },
            id: outcome.id,
            path: outcome.path,
        }
    }
}

impl From<crate::visual_index::VisualAsset> for FfiVisualAsset {
    fn from(asset: crate::visual_index::VisualAsset) -> Self {
        Self {
            reading_id: asset.reading_id,
            title: asset.title,
            relative_path: asset.relative_path,
            absolute_file_path: asset.absolute_file_path,
            content_hash: asset.content_hash,
        }
    }
}

impl From<crate::visual_index::VisualAnalysisTask> for FfiVisualAnalysisTask {
    fn from(task: crate::visual_index::VisualAnalysisTask) -> Self {
        Self {
            reading_id: task.reading_id,
            relative_path: task.relative_path,
            absolute_file_path: task.absolute_file_path,
            content_hash: task.content_hash,
            analyzer_version: task.analyzer_version,
        }
    }
}

impl From<FfiVisualAnalysisTask> for crate::visual_index::VisualAnalysisTask {
    fn from(task: FfiVisualAnalysisTask) -> Self {
        Self {
            reading_id: task.reading_id,
            relative_path: task.relative_path,
            absolute_file_path: task.absolute_file_path,
            content_hash: task.content_hash,
            analyzer_version: task.analyzer_version,
        }
    }
}

impl From<FfiVisualAnalysisResult> for crate::visual_index::VisualAnalysisResult {
    fn from(result: FfiVisualAnalysisResult) -> Self {
        Self {
            supported: result.supported,
            labels: result
                .labels
                .into_iter()
                .map(|label| crate::visual_index::VisualLabel {
                    identifier: label.identifier,
                    confidence: label.confidence,
                })
                .collect(),
            palette: result
                .palette
                .into_iter()
                .map(|color| crate::visual_index::WeightedColor {
                    red: color.red,
                    green: color.green,
                    blue: color.blue,
                    weight: color.weight,
                })
                .collect(),
        }
    }
}

impl From<FfiPredominantColor> for crate::visual_index::PredominantColor {
    fn from(color: FfiPredominantColor) -> Self {
        match color {
            FfiPredominantColor::Red => Self::Red,
            FfiPredominantColor::Orange => Self::Orange,
            FfiPredominantColor::Yellow => Self::Yellow,
            FfiPredominantColor::Green => Self::Green,
            FfiPredominantColor::Blue => Self::Blue,
            FfiPredominantColor::Purple => Self::Purple,
            FfiPredominantColor::Pink => Self::Pink,
            FfiPredominantColor::Brown => Self::Brown,
            FfiPredominantColor::Black => Self::Black,
            FfiPredominantColor::Gray => Self::Gray,
            FfiPredominantColor::White => Self::White,
        }
    }
}

impl From<crate::list::ViewCounts> for FfiViewCounts {
    fn from(c: crate::list::ViewCounts) -> Self {
        Self {
            all: c.all,
            unread: c.unread,
            read: c.read,
            archive: c.archive,
            favorites: c.favorites,
        }
    }
}

impl From<crate::list::SidebarCounts> for FfiSidebarCounts {
    fn from(c: crate::list::SidebarCounts) -> Self {
        Self {
            views: c.views.into(),
            tags: c
                .tags
                .into_iter()
                .map(|(tag, count)| FfiTagCount { tag, count })
                .collect(),
            ratings: c
                .ratings
                .into_iter()
                .map(|(rating, count)| FfiRatingCount { rating, count })
                .collect(),
        }
    }
}

impl From<FfiView> for View {
    fn from(v: FfiView) -> Self {
        match v {
            FfiView::All => View::All,
            FfiView::Unread => View::Unread,
            FfiView::Read => View::Read,
            FfiView::Archive => View::Archive,
            FfiView::Favorites => View::Favorites,
            FfiView::Media => View::Media,
            FfiView::Articles => View::Articles,
            FfiView::Notes => View::Notes,
            FfiView::Links => View::Links,
            FfiView::Quotes => View::Quotes,
        }
    }
}

impl From<FfiCountScope> for CountScope {
    fn from(s: FfiCountScope) -> Self {
        Self {
            view: s.view.into(),
            tag: s.tag,
            rating: s.rating,
            kind: s.kind.map(Into::into),
            query: s.query,
            tag_terms: s.tag_terms,
            visual_terms: s.visual_terms,
            predominant_color: s.predominant_color.map(Into::into),
            semantic_candidate_ids: s.semantic_candidate_ids,
            visual_semantic_candidate_ids: s.visual_semantic_candidate_ids,
        }
    }
}

impl From<FfiListOptions> for ListOptions {
    fn from(o: FfiListOptions) -> Self {
        Self {
            view: o.view.into(),
            sort: match o.sort {
                FfiSortField::SavedAt => SortField::SavedAt,
                FfiSortField::ReadAt => SortField::ReadAt,
                FfiSortField::Rating => SortField::Rating,
                FfiSortField::WordCount => SortField::WordCount,
                FfiSortField::Relevance => SortField::Relevance,
            },
            ascending: o.ascending,
            tag: o.tag,
            rating: o.rating,
            kind: o.kind.map(Into::into),
            since: o.since,
            until: o.until,
            query: o.query,
            tag_terms: o.tag_terms,
            visual_terms: o.visual_terms,
            color_terms: o.color_terms,
            item_type_terms: o.item_type_terms,
            predominant_color: o.predominant_color.map(Into::into),
            semantic_candidate_ids: o.semantic_candidate_ids,
            visual_semantic_candidate_ids: o.visual_semantic_candidate_ids,
            limit: o.limit as usize,
            offset: o.offset as usize,
        }
    }
}

// ── Database object ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReconciliationMode {
    Rebuild,
    Sync,
}

/// Work in progress lives only in memory. Until the final batch succeeds, the
/// persistent index and the last complete scan remain untouched.
struct ReconciliationSession {
    id: u64,
    mode: ReconciliationMode,
    library: LibraryRoot,
    directories: Vec<PathBuf>,
    next: usize,
    readings: Vec<ScannedReading>,
    epoch: u64,
}

/// The main entry point for the Swift client.
///
/// Wraps the SQLite connection, its sibling visual snapshot cache, and the
/// last-known library scan so `sync()` can incrementally reconcile after the
/// initial `rebuild()`.
#[derive(uniffi::Object)]
pub struct Database {
    conn: Mutex<rusqlite::Connection>,
    last_scan: Mutex<Vec<ScannedReading>>,
    reconciliation: Mutex<()>,
    reconciliation_session: Mutex<Option<ReconciliationSession>>,
    reconciliation_epoch: AtomicU64,
    next_reconciliation_session_id: AtomicU64,
    scan_initialized: AtomicBool,
    visual_cache_root: PathBuf,
    // Staging/pruning may wait on slow storage. Serialize those operations
    // separately so interactive database reads never wait on their filesystem I/O.
    visual_cache_io: Mutex<()>,
}

#[uniffi::export]
impl Database {
    /// Open (or create) the index at `db_path`.
    #[uniffi::constructor]
    pub fn open(db_path: String) -> Result<Arc<Self>, CoreError> {
        let db_path = Path::new(&db_path);
        let conn = crate::open_index(db_path).map_err(e)?;
        let visual_cache_root = crate::visual_index::prepare_visual_cache(db_path).map_err(e)?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            last_scan: Mutex::new(Vec::new()),
            reconciliation: Mutex::new(()),
            reconciliation_session: Mutex::new(None),
            reconciliation_epoch: AtomicU64::new(0),
            next_reconciliation_session_id: AtomicU64::new(1),
            scan_initialized: AtomicBool::new(false),
            visual_cache_root,
            visual_cache_io: Mutex::new(()),
        }))
    }

    // ── Indexing ──────────────────────────────────────────────────────────

    /// Prepare a full rebuild without touching the currently visible index.
    /// `rebuild_batch` inspects at most `max_readings` folders per call and
    /// publishes the complete result only when the final batch succeeds.
    pub fn begin_rebuild(&self, library_path: String) -> Result<u64, CoreError> {
        self.begin_reconciliation(library_path, ReconciliationMode::Rebuild)
    }

    /// Returns true after the complete scan has been published atomically.
    pub fn rebuild_batch(&self, session_id: u64, max_readings: u32) -> Result<bool, CoreError> {
        self.reconciliation_batch(session_id, ReconciliationMode::Rebuild, max_readings)
            .map(|result| result.is_some())
    }

    /// Prepare a full recovery scan without touching the currently visible
    /// index. This also detects unreported additions and removals.
    pub fn begin_sync(&self, library_path: String) -> Result<u64, CoreError> {
        self.begin_reconciliation(library_path, ReconciliationMode::Sync)
    }

    /// Returns the diff count when the complete scan has been applied.
    pub fn sync_batch(&self, session_id: u64, max_readings: u32) -> Result<Option<u32>, CoreError> {
        self.reconciliation_batch(session_id, ReconciliationMode::Sync, max_readings)
    }

    /// Discard staged work after cancellation or failure. No index rows have
    /// been changed by an incomplete session.
    pub fn abort_reconciliation(&self, session_id: u64) {
        let _update = self.reconciliation.lock().unwrap();
        let mut session = self.reconciliation_session.lock().unwrap();
        if session
            .as_ref()
            .is_some_and(|current| current.id == session_id)
        {
            *session = None;
        }
    }

    /// Full rebuild: scan the library and repopulate the index from scratch.
    ///
    /// Call this on first launch or after the library folder is replaced.
    /// Stores the resulting scan snapshot so `sync` can diff against it.
    pub fn rebuild(&self, library_path: String) -> Result<(), CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let scan = crate::scan_library(&lib).map_err(e)?;
        {
            let conn = self.conn.lock().unwrap();
            crate::reconcile::rebuild_scanned(&conn, &scan).map_err(e)?;
        }
        self.prune_visual_cache()?;
        *self.last_scan.lock().unwrap() = scan;
        self.scan_initialized.store(true, Ordering::Release);
        Ok(())
    }

    /// Incremental sync: diff the current library against the stored snapshot
    /// and apply only the changes. Returns the number of diffs applied.
    ///
    /// Call this on subsequent launches or when a file-system watch fires.
    pub fn sync(&self, library_path: String) -> Result<u32, CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let new_scan = crate::scan_library(&lib).map_err(e)?;
        self.sync_scanned(new_scan)
    }

    /// Reconcile the reading folders affected by precise filesystem events.
    /// An uninitialized snapshot or an ambiguous path falls back to a full scan.
    pub fn sync_paths(
        &self,
        library_path: String,
        changed_paths: Vec<String>,
    ) -> Result<u32, CoreError> {
        let has_events = !changed_paths.is_empty();
        match self.sync_paths_if_precise(library_path.clone(), changed_paths)? {
            Some(count) => Ok(count),
            None => self
                .sync(library_path)
                .map(|changed| changed.max(u32::from(has_events))),
        }
    }

    /// Reconcile precise file events only. `None` requests a full recovery
    /// scan, which a UI can run in paced batches instead of blocking one call.
    pub fn sync_paths_if_precise(
        &self,
        library_path: String,
        changed_paths: Vec<String>,
    ) -> Result<Option<u32>, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let ids = crate::scanner::reading_ids_for_changed_paths(&lib, &changed_paths);
        let Some(ids) = ids.filter(|_| self.scan_initialized.load(Ordering::Acquire)) else {
            return Ok(None);
        };
        if ids.is_empty() {
            return Ok(Some(0));
        }
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let new_scan = crate::scanner::scan_reading_ids(&lib, &ids).map_err(e)?;
        let diffs = {
            let old_scan = self.last_scan.lock().unwrap();
            let affected: Vec<_> = old_scan
                .iter()
                .filter(|reading| ids.contains(&reading.id))
                .cloned()
                .collect();
            crate::diff(&affected, &new_scan)
        };
        {
            let conn = self.conn.lock().unwrap();
            crate::apply_diffs(&conn, &diffs).map_err(e)?;
        }
        let mut previous = self.last_scan.lock().unwrap();
        previous.retain(|reading| !ids.contains(&reading.id));
        previous.extend(new_scan);
        // Cache pruning belongs to the next visual reconciliation, rather than
        // walking every cached image for a one-reading filesystem event.
        Ok(Some((diffs.len() as u32).max(u32::from(
            crate::scanner::changed_paths_include_assets(&lib, &changed_paths),
        ))))
    }

    /// Safely enumerate current preview assets for Core Spotlight donation.
    pub fn current_visual_assets_batch(
        &self,
        library_path: String,
        after_reading_id: Option<String>,
        limit: u32,
    ) -> Result<FfiVisualAssetsBatch, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let limit = limit.clamp(1, 64) as usize;
        let _io = self.visual_cache_io.lock().unwrap();
        let (active, candidates) = {
            let conn = self.conn.lock().unwrap();
            (
                if after_reading_id.is_none() {
                    Some(crate::visual_index::active_visual_hashes(&conn).map_err(e)?)
                } else {
                    None
                },
                crate::visual_index::visual_asset_candidate_batch(
                    &conn,
                    after_reading_id.as_deref(),
                    limit,
                )
                .map_err(e)?,
            )
        };
        let next_reading_id =
            (candidates.len() == limit).then(|| candidates.last().unwrap().0.clone());
        if let Some(active) = active {
            crate::visual_index::prune_visual_cache_files(&self.visual_cache_root, &active)
                .map_err(e)?;
        }
        let assets =
            crate::visual_index::stage_visual_assets(candidates, &lib, &self.visual_cache_root)
                .map_err(e)?
                .into_iter()
                .map(Into::into)
                .collect();
        Ok(FfiVisualAssetsBatch {
            assets,
            next_reading_id,
        })
    }

    /// Safely enumerate current preview assets for Core Spotlight donation.
    pub fn current_visual_assets(
        &self,
        library_path: String,
    ) -> Result<Vec<FfiVisualAsset>, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let _io = self.visual_cache_io.lock().unwrap();
        let (active, candidates) = {
            let conn = self.conn.lock().unwrap();
            (
                crate::visual_index::active_visual_hashes(&conn).map_err(e)?,
                crate::visual_index::current_visual_asset_candidates(&conn).map_err(e)?,
            )
        };
        crate::visual_index::prune_visual_cache_files(&self.visual_cache_root, &active)
            .map_err(e)?;
        crate::visual_index::stage_visual_assets(candidates, &lib, &self.visual_cache_root)
            .map_err(e)
            .map(|assets| assets.into_iter().map(Into::into).collect())
    }

    /// Return at most `limit` immutable staged snapshots not cached for this
    /// analyzer version. Exact analysis hits are applied without being retried.
    pub fn pending_visual_analysis_batch(
        &self,
        library_path: String,
        analyzer_version: String,
        after_reading_id: Option<String>,
        limit: u32,
    ) -> Result<FfiVisualAnalysisBatch, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let _io = self.visual_cache_io.lock().unwrap();
        let (candidates, hydrated_count, next_reading_id) = {
            let conn = self.conn.lock().unwrap();
            crate::visual_index::pending_visual_candidate_batch(
                &conn,
                &analyzer_version,
                after_reading_id.as_deref(),
                limit.clamp(1, 64) as usize,
            )
            .map_err(e)?
        };
        let pending = crate::visual_index::stage_visual_analysis(
            candidates,
            hydrated_count,
            &lib,
            &self.visual_cache_root,
            &analyzer_version,
        )
        .map_err(e)?;
        Ok(FfiVisualAnalysisBatch {
            tasks: pending.tasks.into_iter().map(Into::into).collect(),
            hydrated_count: hydrated_count as u32,
            next_reading_id,
        })
    }

    /// Return at most `limit` immutable staged snapshots not cached for this
    /// analyzer version. Exact analysis hits are applied without being retried.
    pub fn pending_visual_analysis(
        &self,
        library_path: String,
        analyzer_version: String,
        limit: u32,
    ) -> Result<FfiPendingVisualAnalysis, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let _io = self.visual_cache_io.lock().unwrap();
        let (active, (candidates, hydrated_count)) = {
            let conn = self.conn.lock().unwrap();
            (
                crate::visual_index::active_visual_hashes(&conn).map_err(e)?,
                crate::visual_index::pending_visual_candidates(
                    &conn,
                    &analyzer_version,
                    limit as usize,
                )
                .map_err(e)?,
            )
        };
        crate::visual_index::prune_visual_cache_files(&self.visual_cache_root, &active)
            .map_err(e)?;
        crate::visual_index::stage_visual_analysis(
            candidates,
            hydrated_count,
            &lib,
            &self.visual_cache_root,
            &analyzer_version,
        )
        .map_err(e)
        .map(|pending| FfiPendingVisualAnalysis {
            tasks: pending.tasks.into_iter().map(Into::into).collect(),
            hydrated_count: pending.hydrated_count.min(u32::MAX as usize) as u32,
        })
    }

    /// Cache a platform result only when its reading, path, and raw bytes still
    /// match the issued task. `false` is an ordinary stale-task outcome.
    pub fn complete_visual_analysis(
        &self,
        library_path: String,
        task: FfiVisualAnalysisTask,
        result: FfiVisualAnalysisResult,
    ) -> Result<bool, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let task = task.into();
        let result = result.into();
        let Some(current) = crate::visual_index::verify_visual_analysis(&lib, &task).map_err(e)?
        else {
            return Ok(false);
        };
        let conn = self.conn.lock().unwrap();
        crate::visual_index::complete_verified_visual_analysis(&conn, &current, &task, &result)
            .map_err(e)
    }

    // ── Imports ───────────────────────────────────────────────────────────

    /// Consume complete Inbox inputs through the shared reading importer.
    /// Does not reconcile the index: callers follow the batch with `sync`.
    pub fn process_inbox(
        &self,
        library_path: String,
        deferred_names: Vec<String>,
    ) -> Result<FfiInboxReport, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let report = crate::process_inbox(&lib, &deferred_names).map_err(e)?;
        Ok(FfiInboxReport {
            saved: report.saved,
            duplicates: report.duplicates,
            pending: report.pending,
            issues: report
                .issues
                .into_iter()
                .map(|issue| FfiInboxIssue {
                    name: issue.name,
                    message: issue.message,
                })
                .collect(),
        })
    }

    pub fn process_inbox_with_instagram(
        &self,
        library_path: String,
        deferred_names: Vec<String>,
        python_path: String,
        script_path: String,
    ) -> Result<FfiInboxReport, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let report = crate::inbox::process_inbox_with_instagram(
            &lib,
            &deferred_names,
            Path::new(&python_path),
            Path::new(&script_path),
        )
        .map_err(e)?;
        Ok(FfiInboxReport {
            saved: report.saved,
            duplicates: report.duplicates,
            pending: report.pending,
            issues: report
                .issues
                .into_iter()
                .map(|issue| FfiInboxIssue {
                    name: issue.name,
                    message: issue.message,
                })
                .collect(),
        })
    }

    /// Save an HTTP(S) URL. Recognised public sources are resolved into full
    /// local articles; ordinary URLs remain lightweight placeholders.
    pub fn import_link(
        &self,
        library_path: String,
        url: String,
    ) -> Result<FfiImportResult, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let outcome = crate::save_url(&lib, crate::UrlSaveRequest::new(url)).map_err(e)?;
        self.sync(library_path)?;
        Ok(outcome.into())
    }

    /// Add source-less plain text as a quote.
    pub fn import_text(
        &self,
        library_path: String,
        text: String,
        title: Option<String>,
    ) -> Result<FfiImportResult, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let outcome = crate::import_text(&lib, &text, title.as_deref()).map_err(e)?;
        self.sync(library_path)?;
        Ok(outcome.into())
    }

    /// Add source-less image bytes. `content_type` determines the local asset
    /// extension and `title` supplies the visible card label.
    pub fn import_image(
        &self,
        library_path: String,
        bytes: Vec<u8>,
        content_type: String,
        title: String,
    ) -> Result<FfiImportResult, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let outcome = crate::import_image(&lib, bytes, &content_type, &title).map_err(e)?;
        self.sync(library_path)?;
        Ok(outcome.into())
    }

    /// Add a source-less local video by path. The core streams the source into
    /// the library while hashing it; video bytes never cross the FFI boundary.
    pub fn import_video_file(
        &self,
        library_path: String,
        file_path: String,
        content_type: String,
        title: String,
    ) -> Result<FfiImportResult, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let outcome = crate::import_video_file(&lib, Path::new(&file_path), &content_type, &title)
            .map_err(e)?;
        self.sync(library_path)?;
        Ok(outcome.into())
    }

    // ── Query ─────────────────────────────────────────────────────────────

    pub fn list_readings(&self, opts: FfiListOptions) -> Result<Vec<FfiReadingRow>, CoreError> {
        let conn = self.conn.lock().unwrap();
        crate::list_readings(&conn, &opts.into())
            .map_err(e)
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    /// Every sidebar count section — the view badges, the tag counts, and the
    /// rating counts — in one call, scoped by the active search and the selected
    /// facets. Resolves the full-text `MATCH` once and returns all three
    /// together; see `sidebar_counts`.
    pub fn sidebar_counts(&self, scope: FfiCountScope) -> Result<FfiSidebarCounts, CoreError> {
        let conn = self.conn.lock().unwrap();
        crate::sidebar_counts(&conn, &scope.into())
            .map_err(e)
            .map(Into::into)
    }

    /// Fetch a reading's metadata row. Returns `None` if not found.
    pub fn get_reading_row(&self, id: String) -> Result<Option<FfiReadingRow>, CoreError> {
        let conn = self.conn.lock().unwrap();
        crate::get_reading(&conn, &id)
            .map_err(e)
            .map(|opt| opt.map(|(row, _body)| row.into()))
    }

    /// Read optional visual attributes and safe local file facts on demand.
    pub fn get_reading_inspector(
        &self,
        library_path: String,
        id: String,
    ) -> Result<Option<crate::inspector::ReadingInspector>, CoreError> {
        let snapshot = {
            let conn = self.conn.lock().unwrap();
            crate::inspector::Snapshot::read(&conn, &id).map_err(e)?
        };
        let library = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        Ok(snapshot.map(|snapshot| snapshot.inspect(&library)))
    }

    /// Fetch the body text of a reading. Returns `None` if not found.
    pub fn get_body(&self, id: String) -> Result<Option<String>, CoreError> {
        let conn = self.conn.lock().unwrap();
        crate::get_reading(&conn, &id)
            .map_err(e)
            .map(|opt| opt.map(|(_row, body)| body))
    }

    // ── Notes ─────────────────────────────────────────────────────────────

    /// Fetch the optional personal Markdown note attached to a reading.
    pub fn get_note(
        &self,
        library_path: String,
        reading_id: String,
    ) -> Result<Option<String>, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        crate::get_note(&lib, &reading_id).map_err(e)
    }

    /// Replace a reading's personal Markdown note. Blank Markdown clears it.
    pub fn set_note(
        &self,
        library_path: String,
        reading_id: String,
        markdown: String,
    ) -> Result<(), CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        crate::set_note(&lib, &reading_id, &markdown).map_err(e)?;
        self.sync(library_path)?;
        Ok(())
    }

    // ── Tags ──────────────────────────────────────────────────────────────

    pub fn add_tag(&self, library_path: String, id: String, tag: String) -> Result<(), CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let conn = self.conn.lock().unwrap();
        crate::add_tag(&lib, &conn, &id, &tag).map_err(e)
    }

    pub fn remove_tag(
        &self,
        library_path: String,
        id: String,
        tag: String,
    ) -> Result<(), CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let conn = self.conn.lock().unwrap();
        crate::remove_tag(&lib, &conn, &id, &tag).map_err(e)
    }

    // ── Ratings ───────────────────────────────────────────────────────────

    /// Set a reading's star rating (0–5, where 0 clears it).
    pub fn set_rating(
        &self,
        library_path: String,
        id: String,
        rating: u8,
    ) -> Result<(), CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let conn = self.conn.lock().unwrap();
        crate::set_rating(&lib, &conn, &id, rating).map_err(e)
    }

    // ── Status flags ──────────────────────────────────────────────────────

    pub fn set_read(&self, library_path: String, id: String, read: bool) -> Result<(), CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let conn = self.conn.lock().unwrap();
        crate::set_read(&lib, &conn, &id, read).map_err(e)
    }

    pub fn set_archived(
        &self,
        library_path: String,
        id: String,
        archived: bool,
    ) -> Result<(), CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let conn = self.conn.lock().unwrap();
        crate::set_archived(&lib, &conn, &id, archived).map_err(e)
    }

    pub fn set_favorite(
        &self,
        library_path: String,
        id: String,
        favorite: bool,
    ) -> Result<(), CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let conn = self.conn.lock().unwrap();
        crate::set_favorite(&lib, &conn, &id, favorite).map_err(e)
    }

    // ── Deletion ──────────────────────────────────────────────────────────

    /// Permanently delete a reading: its file, assets, and index row. Unlike
    /// `set_archived`, this cannot be undone.
    pub fn delete_reading(&self, library_path: String, id: String) -> Result<(), CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        self.reconciliation_epoch.fetch_add(1, Ordering::AcqRel);
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let conn = self.conn.lock().unwrap();
        crate::delete_reading(&lib, &conn, &id).map_err(e)
    }

    // ── Highlights ────────────────────────────────────────────────────────

    /// List a reading's saved highlights, in creation order.
    pub fn list_highlights(
        &self,
        library_path: String,
        reading_id: String,
    ) -> Result<Vec<FfiHighlight>, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        crate::list_highlights(&lib, &reading_id)
            .map_err(e)
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    /// Save a new highlight (the verbatim selected text) for a reading and
    /// return it. Re-adding an existing passage is a no-op that returns the
    /// existing highlight.
    pub fn add_highlight(
        &self,
        library_path: String,
        reading_id: String,
        text: String,
    ) -> Result<FfiHighlight, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        crate::add_highlight(&lib, &reading_id, &text)
            .map_err(e)
            .map(Into::into)
    }

    /// Toggle a highlight by its text: removes it if the exact passage is
    /// already highlighted, otherwise adds it. Returns `true` if the passage is
    /// highlighted after the call, `false` if it was cleared.
    pub fn toggle_highlight(
        &self,
        library_path: String,
        reading_id: String,
        text: String,
    ) -> Result<bool, CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        crate::toggle_highlight(&lib, &reading_id, &text).map_err(e)
    }

    /// Remove a single highlight from a reading.
    pub fn delete_highlight(
        &self,
        library_path: String,
        reading_id: String,
        highlight_id: String,
    ) -> Result<(), CoreError> {
        let lib = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        crate::delete_highlight(&lib, &reading_id, &highlight_id).map_err(e)
    }
}

impl Database {
    fn begin_reconciliation(
        &self,
        library_path: String,
        mode: ReconciliationMode,
    ) -> Result<u64, CoreError> {
        let _update = self.reconciliation.lock().unwrap();
        let library = LibraryRoot::new(Path::new(&library_path)).map_err(e)?;
        let directories = crate::scanner::reading_directories(&library).map_err(e)?;
        let id = self
            .next_reconciliation_session_id
            .fetch_add(1, Ordering::AcqRel);
        *self.reconciliation_session.lock().unwrap() = Some(ReconciliationSession {
            id,
            mode,
            library,
            directories,
            next: 0,
            readings: Vec::new(),
            epoch: self.reconciliation_epoch.load(Ordering::Acquire),
        });
        Ok(id)
    }

    fn reconciliation_batch(
        &self,
        session_id: u64,
        mode: ReconciliationMode,
        max_readings: u32,
    ) -> Result<Option<u32>, CoreError> {
        if max_readings == 0 {
            return Err(e("reconciliation batch size must be positive"));
        }
        let _update = self.reconciliation.lock().unwrap();
        let mut pending = self.reconciliation_session.lock().unwrap();
        let Some(current) = pending.as_ref() else {
            return Err(e("reconciliation session is not active"));
        };
        if current.id != session_id || current.mode != mode {
            return Err(e("reconciliation session was replaced"));
        }
        let mut session = pending.take().unwrap();
        drop(pending);

        // A same-Database writer or direct sync may have committed between
        // batches. Re-enumerate and start over rather than publishing older
        // scanned rows on top of that mutation. External writers are covered
        // by the caller's watcher and subsequent full recovery scan.
        let epoch = self.reconciliation_epoch.load(Ordering::Acquire);
        if epoch != session.epoch {
            session.directories =
                crate::scanner::reading_directories(&session.library).map_err(e)?;
            session.next = 0;
            session.readings.clear();
            session.epoch = epoch;
        }

        let end = session
            .next
            .saturating_add(max_readings as usize)
            .min(session.directories.len());
        for directory in &session.directories[session.next..end] {
            if let Some(reading) =
                crate::scanner::scan_reading_directory(&session.library, directory).map_err(e)?
            {
                session.readings.push(reading);
            }
        }
        session.next = end;
        if end < session.directories.len() {
            *self.reconciliation_session.lock().unwrap() = Some(session);
            return Ok(None);
        }

        // No partial rows were published. A failed final transaction leaves
        // the previous index intact and discards this session.
        match mode {
            ReconciliationMode::Rebuild => {
                {
                    let conn = self.conn.lock().unwrap();
                    crate::reconcile::rebuild_scanned(&conn, &session.readings).map_err(e)?;
                }
                *self.last_scan.lock().unwrap() = session.readings;
                self.scan_initialized.store(true, Ordering::Release);
                self.prune_visual_cache()?;
                Ok(Some(0))
            }
            ReconciliationMode::Sync => self.sync_scanned(session.readings).map(Some),
        }
    }

    /// The caller holds `reconciliation` while applying a complete snapshot.
    fn sync_scanned(&self, new_scan: Vec<ScannedReading>) -> Result<u32, CoreError> {
        let mut diffs = {
            let old_scan = self.last_scan.lock().unwrap();
            crate::diff(&old_scan, &new_scan)
        };
        if !self.scan_initialized.load(Ordering::Acquire) {
            // A reopened Database has no previous filesystem snapshot. Rows
            // that disappeared while it was closed still need removal from
            // the persistent disposable index on its first reconciliation.
            let present: std::collections::HashSet<_> =
                new_scan.iter().map(|reading| reading.id.as_str()).collect();
            let conn = self.conn.lock().unwrap();
            let mut statement = conn.prepare("SELECT id FROM readings").map_err(e)?;
            for id in statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(e)?
            {
                let id = id.map_err(e)?;
                if !present.contains(id.as_str()) {
                    diffs.push(crate::scanner::ScanDiff::Removed(id));
                }
            }
        }
        let count = diffs.len() as u32;
        if !diffs.is_empty() {
            let conn = self.conn.lock().unwrap();
            crate::apply_diffs(&conn, &diffs).map_err(e)?;
        }
        *self.last_scan.lock().unwrap() = new_scan;
        self.scan_initialized.store(true, Ordering::Release);
        if !diffs.is_empty() {
            self.prune_visual_cache()?;
        }
        Ok(count)
    }

    fn prune_visual_cache(&self) -> Result<(), CoreError> {
        let _io = self.visual_cache_io.lock().unwrap();
        let active = {
            let conn = self.conn.lock().unwrap();
            crate::visual_index::active_visual_hashes(&conn).map_err(e)?
        };
        crate::visual_index::prune_visual_cache_files(&self.visual_cache_root, &active).map_err(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portrait_png() -> Vec<u8> {
        let mut bytes = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        bytes.extend_from_slice(&1900_u32.to_be_bytes());
        bytes.extend_from_slice(&2468_u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn list_options(view: FfiView) -> FfiListOptions {
        FfiListOptions {
            view,
            sort: FfiSortField::SavedAt,
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

    #[test]
    fn inbox_batch_is_reconciled_once_by_the_caller() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let inbox = library_dir.path().join("inbox");
        std::fs::create_dir(&inbox).unwrap();
        let path = inbox.join("link.txt");
        std::fs::write(&path, "https://example.com/from-ios").unwrap();
        std::fs::File::open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(10))
            .unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let library_path = library_dir.path().display().to_string();

        let deferred = database
            .process_inbox(library_path.clone(), vec!["link.txt".to_string()])
            .unwrap();
        assert_eq!(deferred.pending, 1);
        assert!(path.exists());

        let report = database
            .process_inbox(library_path.clone(), vec![])
            .unwrap();
        assert_eq!(report.saved, 1);
        assert!(report.issues.is_empty());
        assert!(!path.exists());
        assert!(database
            .list_readings(list_options(FfiView::All))
            .unwrap()
            .is_empty());

        assert_eq!(database.sync(library_path.clone()).unwrap(), 1);
        let rows = database.list_readings(list_options(FfiView::All)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "https://example.com/from-ios");
        assert!(rows[0].lightweight);
        assert_eq!(database.sync(library_path).unwrap(), 0);
    }

    #[test]
    fn path_based_video_import_is_indexed_without_a_byte_buffer() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let source_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let source_path = source_dir.path().join("clip.mp4");
        std::fs::write(&source_path, b"file-backed ffi video").unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();

        let imported = database
            .import_video_file(
                library_dir.path().display().to_string(),
                source_path.display().to_string(),
                "video/mp4".to_string(),
                "Clip".to_string(),
            )
            .unwrap();
        let row = database.get_reading_row(imported.id).unwrap().unwrap();

        assert!(matches!(imported.disposition, FfiImportDisposition::Saved));
        assert!(matches!(row.kind, FfiReadingKind::Video));
        assert!(row
            .media_url
            .as_deref()
            .is_some_and(|url| url.starts_with("cuttings-asset:assets/") && url.ends_with(".mp4")));
    }

    #[test]
    fn visual_asset_paths_are_staged_beside_the_database() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let imported = database
            .import_image(
                library_dir.path().display().to_string(),
                b"ffi staged image".to_vec(),
                "image/png".into(),
                "Staged".into(),
            )
            .unwrap();

        let asset = database
            .current_visual_assets(library_dir.path().display().to_string())
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(asset.reading_id, imported.id);
        assert!(Path::new(&asset.absolute_file_path)
            .starts_with(index_dir.path().join("visual-assets")));
        assert_eq!(
            std::fs::read(asset.absolute_file_path).unwrap(),
            b"ffi staged image"
        );
    }

    #[test]
    fn visual_filesystem_work_does_not_block_reading_queries() {
        use std::{sync::mpsc, thread, time::Duration};

        for phase in [0, 1, 2] {
            let library_dir = tempfile::TempDir::new().unwrap();
            let index_dir = tempfile::TempDir::new().unwrap();
            let database =
                Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
            let library_path = library_dir.path().display().to_string();
            let imported = database
                .import_image(
                    library_path.clone(),
                    b"slow staging image".to_vec(),
                    "image/png".into(),
                    "Available while indexing".into(),
                )
                .unwrap();
            let completion_task = database
                .pending_visual_analysis(library_path.clone(), "test-v1".into(), 1)
                .unwrap()
                .tasks
                .pop()
                .unwrap();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let worker_database = Arc::clone(&database);
            let worker = thread::spawn(move || {
                crate::visual_index::with_visual_io_test_hook(
                    move || {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    },
                    || match phase {
                        0 => worker_database
                            .current_visual_assets(library_path)
                            .map(|value| value.len()),
                        1 => worker_database
                            .pending_visual_analysis(library_path, "test-v1".into(), 1)
                            .map(|value| value.tasks.len()),
                        _ => worker_database
                            .complete_visual_analysis(
                                library_path,
                                completion_task,
                                FfiVisualAnalysisResult {
                                    supported: false,
                                    labels: vec![],
                                    palette: vec![],
                                },
                            )
                            .map(usize::from),
                    },
                )
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let (read_tx, read_rx) = mpsc::channel();
            let reader = thread::spawn(move || {
                read_tx.send(database.get_reading_row(imported.id)).unwrap();
            });
            let result = read_rx.recv_timeout(Duration::from_secs(1));
            // Always release/join the workers before asserting a failed deadline.
            release_tx.send(()).unwrap();
            assert_eq!(worker.join().unwrap().unwrap(), 1);
            reader.join().unwrap();
            assert_eq!(
                result
                    .expect("reading query waited for filesystem staging")
                    .unwrap()
                    .unwrap()
                    .title,
                "Available while indexing"
            );
        }
    }

    #[test]
    fn visual_asset_batches_stage_only_the_requested_window() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let library_path = library_dir.path().display().to_string();
        let mut expected = Vec::new();
        for image in 0..5 {
            expected.push(
                database
                    .import_image(
                        library_path.clone(),
                        format!("image-{image}").into_bytes(),
                        "image/png".into(),
                        format!("Image {image}"),
                    )
                    .unwrap()
                    .id,
            );
        }
        expected.sort();
        let staged = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&staged);
        let first = crate::visual_index::with_visual_io_test_hook(
            move || {
                counter.fetch_add(1, Ordering::Relaxed);
            },
            || database.current_visual_assets_batch(library_path.clone(), None, 2),
        )
        .unwrap();
        assert_eq!(
            staged.load(Ordering::Relaxed),
            2,
            "pagination must bound filesystem work, not just truncate the result"
        );
        let mut actual: Vec<_> = first
            .assets
            .into_iter()
            .map(|asset| asset.reading_id)
            .collect();
        let mut cursor = first.next_reading_id;
        while let Some(after) = cursor {
            let batch = database
                .current_visual_assets_batch(library_path.clone(), Some(after), 2)
                .unwrap();
            assert!(batch.assets.len() <= 2);
            actual.extend(batch.assets.into_iter().map(|asset| asset.reading_id));
            cursor = batch.next_reading_id;
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn pending_visual_batches_advance_past_unreadable_assets() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let mut ids = Vec::new();
        for image in ["broken", "available"] {
            ids.push(
                database
                    .import_image(
                        library_path.clone(),
                        image.as_bytes().to_vec(),
                        "image/png".into(),
                        image.into(),
                    )
                    .unwrap()
                    .id,
            );
        }
        ids.sort();
        let first = database.get_reading_row(ids[0].clone()).unwrap().unwrap();
        std::fs::remove_file(
            library
                .reading_dir(&ids[0])
                .join(first.preview_asset.unwrap()),
        )
        .unwrap();
        let skipped = database
            .pending_visual_analysis_batch(library_path.clone(), "test-v1".into(), None, 1)
            .unwrap();
        assert!(skipped.tasks.is_empty());
        assert_eq!(
            skipped.next_reading_id,
            Some(ids[0].clone()),
            "a failed stage must still advance the scan cursor"
        );
        let next = database
            .pending_visual_analysis_batch(
                library_path,
                "test-v1".into(),
                skipped.next_reading_id,
                1,
            )
            .unwrap();
        assert_eq!(next.tasks.len(), 1);
        assert_eq!(next.tasks[0].reading_id, ids[1]);
    }

    #[test]
    fn a_scanned_snapshot_cannot_overwrite_a_later_tag_edit() {
        use std::{sync::mpsc, thread, time::Duration};
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let id = database
            .import_image(
                library_path.clone(),
                b"scan gate image".to_vec(),
                "image/png".into(),
                "Before".into(),
            )
            .unwrap()
            .id;
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let article = library.article_path(&id);
        std::fs::write(
            &article,
            std::fs::read_to_string(&article)
                .unwrap()
                .replace("Before", "External"),
        )
        .unwrap();

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let scanner_database = Arc::clone(&database);
        let scanner_path = library_path.clone();
        let scanner = thread::spawn(move || {
            crate::visual_index::with_visual_io_test_hook(
                move || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                },
                || scanner_database.sync(scanner_path),
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // Cached queries remain available while the filesystem scan is paused.
        assert_eq!(
            database.get_reading_row(id.clone()).unwrap().unwrap().title,
            "Before"
        );
        let (edited_tx, edited_rx) = mpsc::channel();
        let writer_database = Arc::clone(&database);
        let writer_id = id.clone();
        let writer = thread::spawn(move || {
            edited_tx
                .send(writer_database.add_tag(library_path, writer_id, "keep".into()))
                .unwrap();
        });
        let early_edit = edited_rx.recv_timeout(Duration::from_millis(100));
        release_tx.send(()).unwrap();
        scanner.join().unwrap().unwrap();
        writer.join().unwrap();
        assert!(
            early_edit.is_err(),
            "file mutation raced ahead of an older scan snapshot"
        );
        edited_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let row = database.get_reading_row(id).unwrap().unwrap();
        assert_eq!(row.title, "External");
        assert_eq!(row.tags, ["keep"]);
    }

    #[test]
    fn path_sync_only_reconciles_reported_readings_until_a_full_recovery_scan() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let first = database
            .import_text(
                library_path.clone(),
                "First body".into(),
                Some("First".into()),
            )
            .unwrap();
        let second = database
            .import_text(
                library_path.clone(),
                "Second body".into(),
                Some("Second".into()),
            )
            .unwrap();
        let article = library.article_path(&first.id);
        std::fs::write(
            &article,
            std::fs::read_to_string(&article)
                .unwrap()
                .replace("First", "Changed"),
        )
        .unwrap();
        // This unreported deletion must be discovered by the explicit recovery
        // scan, not by rereading every unrelated folder for the first event.
        std::fs::remove_file(library.article_path(&second.id)).unwrap();
        assert_eq!(
            database
                .sync_paths(library_path.clone(), vec![article.display().to_string()])
                .unwrap(),
            1
        );
        assert_eq!(
            database.get_reading_row(first.id).unwrap().unwrap().title,
            "Changed"
        );
        assert!(database
            .get_reading_row(second.id.clone())
            .unwrap()
            .is_some());
        assert_eq!(database.sync(library_path).unwrap(), 1);
        assert!(database.get_reading_row(second.id).unwrap().is_none());
    }

    #[test]
    fn precise_path_api_defers_ambiguous_events_to_a_full_scan() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let saved = database
            .import_text(library_path.clone(), "body".into(), Some("Before".into()))
            .unwrap();
        let article = library.article_path(&saved.id);
        std::fs::write(
            &article,
            std::fs::read_to_string(&article)
                .unwrap()
                .replace("Before", "After"),
        )
        .unwrap();

        assert_eq!(
            database
                .sync_paths_if_precise(
                    library_path.clone(),
                    vec![library.articles_dir().display().to_string()]
                )
                .unwrap(),
            None
        );
        assert_eq!(
            database
                .get_reading_row(saved.id.clone())
                .unwrap()
                .unwrap()
                .title,
            "Before"
        );
        assert_eq!(
            database
                .sync_paths_if_precise(library_path, vec![article.display().to_string()])
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            database.get_reading_row(saved.id).unwrap().unwrap().title,
            "After"
        );
    }

    #[test]
    fn path_sync_indexes_external_body_edits_without_rewriting_frontmatter() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let imported = database
            .import_text(
                library_path.clone(),
                "initialbodyword".into(),
                Some("External editor".into()),
            )
            .unwrap();
        let article = library.article_path(&imported.id);
        let original = std::fs::read_to_string(&article).unwrap();
        let (frontmatter, _) = original.split_once("\n---\n").unwrap();
        let replacement = format!("{frontmatter}\n---\n\nreplacementbodyword\n");
        std::fs::write(&article, replacement).unwrap();
        let search = |query: &str| {
            let mut options = list_options(FfiView::All);
            options.query = Some(query.into());
            database.list_readings(options).unwrap()
        };
        assert!(search("replacementbodyword").is_empty());
        assert_eq!(
            database
                .sync_paths(library_path, vec![article.display().to_string()])
                .unwrap(),
            1
        );
        let matches = search("replacementbodyword");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, imported.id);
    }

    #[test]
    fn path_sync_recovers_a_new_session_and_ambiguous_ancestor_events() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let database_path = index_dir.path().join("index.db").display().to_string();
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let database = Database::open(database_path.clone()).unwrap();
        let first = database
            .import_text(library_path.clone(), "one".into(), Some("First".into()))
            .unwrap();
        let second = database
            .import_text(library_path.clone(), "two".into(), Some("Second".into()))
            .unwrap();
        drop(database);
        std::fs::remove_file(library.article_path(&second.id)).unwrap();
        let reopened = Database::open(database_path).unwrap();
        reopened
            .sync_paths(
                library_path.clone(),
                vec![library.article_path(&first.id).display().to_string()],
            )
            .unwrap();
        assert!(
            reopened.get_reading_row(second.id).unwrap().is_none(),
            "a fresh session must reconcile the whole index"
        );
        std::fs::remove_file(library.article_path(&first.id)).unwrap();
        assert_eq!(
            reopened
                .sync_paths(
                    library_path,
                    vec![library.articles_dir().display().to_string()]
                )
                .unwrap(),
            1
        );
        assert!(reopened.get_reading_row(first.id).unwrap().is_none());
    }

    #[test]
    fn path_sync_invalidates_replaced_video_bytes_without_hashing_the_movie() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let source = index_dir.path().join("movie.mp4");
        std::fs::write(&source, b"original local movie").unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let library_path = library_dir.path().display().to_string();
        let imported = database
            .import_video_file(
                library_path.clone(),
                source.display().to_string(),
                "video/mp4".into(),
                "Movie".into(),
            )
            .unwrap();
        let row = database
            .get_reading_row(imported.id.clone())
            .unwrap()
            .unwrap();
        assert!(row.preview_asset.is_none());
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let movie = library.reading_dir(&imported.id).join(
            row.media_url
                .unwrap()
                .strip_prefix("cuttings-asset:")
                .unwrap(),
        );
        std::fs::write(&movie, b"replacement local movie").unwrap();
        assert!(
            database
                .sync_paths(library_path, vec![movie.display().to_string()])
                .unwrap()
                > 0,
            "asset events must invalidate presentation even when indexed metadata is unchanged"
        );
    }

    #[test]
    fn staged_rebuild_publishes_only_after_last_batch_and_abort_preserves_index() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        database
            .import_text(library_path.clone(), "one".into(), Some("First".into()))
            .unwrap();
        crate::import_text(&library, "two", Some("Second")).unwrap();

        let abandoned = database.begin_rebuild(library_path.clone()).unwrap();
        assert!(!database.rebuild_batch(abandoned, 1).unwrap());
        assert_eq!(
            database
                .list_readings(list_options(FfiView::All))
                .unwrap()
                .len(),
            1
        );
        database.abort_reconciliation(abandoned);
        assert_eq!(
            database
                .list_readings(list_options(FfiView::All))
                .unwrap()
                .len(),
            1
        );

        let session = database.begin_rebuild(library_path).unwrap();
        database.abort_reconciliation(abandoned);
        assert!(!database.rebuild_batch(session, 1).unwrap());
        assert_eq!(
            database
                .list_readings(list_options(FfiView::All))
                .unwrap()
                .len(),
            1
        );
        assert!(database.rebuild_batch(session, 1).unwrap());
        assert_eq!(
            database
                .list_readings(list_options(FfiView::All))
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn staged_sync_restarts_after_same_database_mutation() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        database
            .import_text(library_path.clone(), "one".into(), Some("First".into()))
            .unwrap();
        database
            .import_text(library_path.clone(), "two".into(), Some("Second".into()))
            .unwrap();

        let session = database.begin_sync(library_path.clone()).unwrap();
        assert_eq!(database.sync_batch(session, 1).unwrap(), None);
        let scanned_id = database
            .reconciliation_session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .readings[0]
            .id
            .clone();
        database
            .add_tag(library_path, scanned_id.clone(), "keep".into())
            .unwrap();
        assert_eq!(database.sync_batch(session, 1).unwrap(), None);
        assert_eq!(database.sync_batch(session, 1).unwrap(), Some(1));
        assert_eq!(
            database.get_reading_row(scanned_id).unwrap().unwrap().tags,
            ["keep"]
        );
    }

    #[test]
    fn full_sync_after_staged_rebuild_recovers_external_mid_scan_changes() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let library = LibraryRoot::new(library_dir.path()).unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        database
            .import_text(library_path.clone(), "one".into(), Some("First".into()))
            .unwrap();
        database
            .import_text(library_path.clone(), "two".into(), Some("Second".into()))
            .unwrap();
        database
            .import_text(library_path.clone(), "three".into(), Some("Third".into()))
            .unwrap();

        let rebuild = database.begin_rebuild(library_path.clone()).unwrap();
        assert!(!database.rebuild_batch(rebuild, 1).unwrap());
        let scanned_id = database
            .reconciliation_session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .readings[0]
            .id
            .clone();
        let old_title = database
            .get_reading_row(scanned_id.clone())
            .unwrap()
            .unwrap()
            .title;
        let article = library.article_path(&scanned_id);
        std::fs::write(
            &article,
            std::fs::read_to_string(&article)
                .unwrap()
                .replace(&old_title, "External"),
        )
        .unwrap();
        assert!(!database.rebuild_batch(rebuild, 1).unwrap());
        let removed_id = database
            .reconciliation_session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .readings[1]
            .id
            .clone();
        std::fs::remove_dir_all(library.reading_dir(&removed_id)).unwrap();
        let added =
            crate::import_text(&library, "new external body", Some("New external")).unwrap();
        assert!(database.rebuild_batch(rebuild, 1).unwrap());
        assert_eq!(
            database
                .get_reading_row(scanned_id.clone())
                .unwrap()
                .unwrap()
                .title,
            old_title
        );
        assert!(database
            .get_reading_row(removed_id.clone())
            .unwrap()
            .is_some());
        assert!(database
            .get_reading_row(added.id.clone())
            .unwrap()
            .is_none());

        let recovery = database.begin_sync(library_path).unwrap();
        assert_eq!(database.sync_batch(recovery, 1).unwrap(), None);
        assert_eq!(database.sync_batch(recovery, 1).unwrap(), None);
        assert_eq!(database.sync_batch(recovery, 1).unwrap(), Some(3));
        assert_eq!(
            database.get_reading_row(scanned_id).unwrap().unwrap().title,
            "External"
        );
        assert!(database.get_reading_row(removed_id).unwrap().is_none());
        assert!(database.get_reading_row(added.id).unwrap().is_some());
    }

    #[test]
    fn media_aspect_ratio_crosses_the_ffi_boundary() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let imported = database
            .import_image(
                library_dir.path().display().to_string(),
                portrait_png(),
                "image/png".into(),
                "Portrait".into(),
            )
            .unwrap();

        let row = database.get_reading_row(imported.id).unwrap().unwrap();
        assert_eq!(row.media_aspect_ratio, Some(1900.0 / 2468.0));
    }

    #[test]
    fn dominant_color_crosses_the_ffi_boundary() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();
        let imported = database
            .import_image(
                library_path.clone(),
                b"ffi palette image".to_vec(),
                "image/png".into(),
                "Palette".into(),
            )
            .unwrap();
        let task = database
            .pending_visual_analysis(library_path.clone(), "vision-r2".into(), 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        assert!(database
            .complete_visual_analysis(
                library_path.clone(),
                task,
                FfiVisualAnalysisResult {
                    supported: true,
                    labels: vec![],
                    palette: vec![FfiWeightedColor {
                        red: 0.2,
                        green: 0.4,
                        blue: 0.8,
                        weight: 1.0,
                    }],
                },
            )
            .unwrap());

        let color = database
            .get_reading_row(imported.id.clone())
            .unwrap()
            .unwrap()
            .dominant_color
            .unwrap();
        assert_eq!(color.red, 0.2);
        assert_eq!(color.green, 0.4);
        assert_eq!(color.blue, 0.8);
        assert_eq!(color.weight, 1.0);

        database
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE readings SET visual_analyzer_version=NULL WHERE id=?1",
                rusqlite::params![imported.id],
            )
            .unwrap();
        let pending = database
            .pending_visual_analysis(library_path, "vision-r2".into(), 1)
            .unwrap();
        assert!(pending.tasks.is_empty());
        assert_eq!(pending.hydrated_count, 1);
    }

    #[test]
    fn new_board_views_cross_the_ffi_boundary() {
        for (ffi, expected) in [
            (FfiView::Media, View::Media),
            (FfiView::Articles, View::Articles),
            (FfiView::Notes, View::Notes),
            (FfiView::Links, View::Links),
            (FfiView::Quotes, View::Quotes),
        ] {
            assert_eq!(View::from(ffi), expected);
        }
    }

    #[test]
    fn note_and_lightweight_flags_are_queryable_through_ffi() {
        let library_dir = tempfile::TempDir::new().unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let library_path = library_dir.path().display().to_string();
        let database =
            Database::open(index_dir.path().join("index.db").display().to_string()).unwrap();

        let imported = database
            .import_link(library_path.clone(), "https://example.com/link".into())
            .unwrap();
        let initial = database
            .get_reading_row(imported.id.clone())
            .unwrap()
            .unwrap();
        assert!(initial.lightweight);
        assert!(!initial.has_note);
        assert!(database
            .list_readings(list_options(FfiView::Notes))
            .unwrap()
            .is_empty());

        database
            .set_note(
                library_path.clone(),
                imported.id.clone(),
                "A personal note".into(),
            )
            .unwrap();
        let noted = database
            .get_reading_row(imported.id.clone())
            .unwrap()
            .unwrap();
        assert!(noted.lightweight);
        assert!(noted.has_note);
        assert_eq!(
            database
                .list_readings(list_options(FfiView::Notes))
                .unwrap()
                .len(),
            1
        );

        database
            .set_note(library_path, imported.id, "  \n".into())
            .unwrap();
        assert!(database
            .list_readings(list_options(FfiView::Notes))
            .unwrap()
            .is_empty());
    }
}
