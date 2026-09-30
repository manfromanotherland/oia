// SPDX-License-Identifier: MIT

//! Shared save and import orchestration.
//!
//! Browser captures and native-app imports both enter the library through this
//! module so identity, deduplication, asset writing, and placeholder upgrades
//! stay identical across clients.

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    find_by_media, find_by_url, first_local_image_asset, media_id, parse_reading, quote_id,
    read_metadata, sha256_hex, url_id, ImageBytes, LibraryRoot, Metadata, Reading, ReadingKind,
    SourceProfile,
};
use crate::{
    images::{
        image_extension, write_images_required_under_lock, write_images_under_lock,
        written_image_asset,
    },
    locking::{lock_reading, ReadingLock},
    notes::set_note_under_lock,
    tags::validate_imported_tag,
    writer::write_reading_under_lock,
};

const LOCAL_ORIGIN_SCHEME: &str = "cuttings://local";
const LOCAL_ASSET_SCHEME: &str = "cuttings-asset:";
// This single directory intentionally persists after its temporary files are
// removed. Deleting a shared staging directory creates a cross-process race:
// one importer can remove it between another importer's create_dir_all and
// create_new calls. Empty operational directories are ignored by the scanner.
const IMPORT_STAGING_DIRECTORY: &str = ".cuttings-imports";
const VIDEO_COPY_BUFFER_SIZE: usize = 1024 * 1024;
pub const MAX_BROWSER_VIDEO_BYTES: u64 = 1024 * 1024 * 1024;
const QUOTE_EXCERPT_CHARACTERS: usize = 600;

/// Everything required to persist one browser capture or native import.
pub struct SaveInput {
    /// Optional canonical quote text used only for identity. Source-less text
    /// supplies its whitespace-normalized content here while retaining the
    /// original line structure in `markdown`; browser captures leave it unset.
    pub quote_identity_markdown: Option<String>,
    pub kind: ReadingKind,
    pub lightweight: bool,
    pub url: String,
    pub media_url: Option<String>,
    pub canonical_url: String,
    pub title: String,
    pub author: Option<String>,
    pub site: Option<String>,
    pub saved_at: String,
    pub markdown: String,
    pub images: Vec<ImageBytes>,
    /// Captured source URL to use as the card preview without adding it to the body.
    pub preview_url: Option<String>,
    /// Captured source URL to retain as the page favicon.
    pub favicon_url: Option<String>,
    /// Website-provided CSS theme color. The core normalizes supported values
    /// to canonical lowercase `#rrggbb` before persisting them.
    pub theme_color: Option<String>,
    pub excerpt: Option<String>,
    pub word_count: Option<u32>,
    pub lang: Option<String>,
}

/// A full article capture enriched with provider-neutral source metadata and
/// required local assets prepared by an adapter.
///
/// The core does not fetch these assets. It verifies and commits them before
/// `article.md`, which remains the visibility/commit marker for the reading.
pub struct SourceCaptureInput {
    pub capture: SaveInput,
    pub source_profile: SourceProfile,
    pub staged_assets: Vec<StagedSourceAsset>,
    /// The article state observed before the adapter started retrieval.
    ///
    /// This makes the later replacement conditional: a source capture never
    /// overwrites an article that appeared or changed while network work was
    /// in flight.
    pub expected_article: ExpectedArticleState,
}

/// File-backed article state observed before a source adapter starts retrieval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedArticleState {
    Missing,
    Sha256(String),
}

/// One temporary local file adopted by [`save_source_capture`].
///
/// Construction validates its portable `assets/<sha256>.<ext>` destination.
/// The declared hash and byte count are verified while committing. Once
/// constructed, the staging file is owned by this value and removed on drop
/// unless it has already been committed and cleaned up.
#[derive(Debug)]
pub struct StagedSourceAsset {
    staging_path: Option<PathBuf>,
    asset: String,
    sha256: String,
    byte_count: u64,
}

impl StagedSourceAsset {
    pub fn new(
        staging_path: PathBuf,
        asset: String,
        sha256: String,
        byte_count: u64,
    ) -> Result<Self, SaveError> {
        let validation = validate_staged_asset_declaration(&asset, &sha256, byte_count);
        if let Err(error) = validation {
            let _ = fs::remove_file(&staging_path);
            return Err(error);
        }
        Ok(Self {
            staging_path: Some(staging_path),
            asset,
            sha256,
            byte_count,
        })
    }

    pub fn asset(&self) -> &str {
        &self.asset
    }
}

impl Drop for StagedSourceAsset {
    fn drop(&mut self) {
        if let Some(path) = self.staging_path.as_ref() {
            let _ = fs::remove_file(path);
        }
    }
}

/// User-controlled state supplied when migrating an existing library item.
///
/// This state is initial state only: it is applied to a newly saved reading,
/// never merged into a duplicate, and never allowed to replace the current
/// state or note during a lightweight-link upgrade.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedReadingState {
    pub favorite: bool,
    pub tags: Vec<String>,
    pub note_markdown: Option<String>,
}

/// Optional source metadata and state shared by the convenience import APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportOptions {
    pub title: Option<String>,
    pub saved_at: Option<String>,
    pub state: ImportedReadingState,
}

/// Metadata supplied before a browser starts streaming a selected video.
pub struct BrowserVideoImportInput {
    pub content_type: String,
    /// Exact decoded byte count when the browser knows it. The completed import
    /// is rejected if the streamed count differs.
    pub expected_bytes: Option<u64>,
    pub origin_url: String,
    pub canonical_url: String,
    pub title: String,
    pub author: Option<String>,
    pub site: Option<String>,
    pub theme_color: Option<String>,
    pub lang: Option<String>,
    pub excerpt: Option<String>,
    pub word_count: Option<u32>,
    pub saved_at: String,
}

/// One in-progress browser video import.
///
/// Bytes are written and hashed incrementally in the library's staging
/// directory. Dropping this value before [`finish`](Self::finish) removes the
/// incomplete file.
pub struct BrowserVideoImport {
    library_path: PathBuf,
    input: Option<BrowserVideoImportInput>,
    extension: String,
    staging_path: Option<PathBuf>,
    destination: Option<fs::File>,
    hasher: Sha256,
    byte_count: u64,
    failed: bool,
}

/// Live-DOM metadata and local image bytes for a lightweight browser link save.
/// The core still owns URL normalization, body construction, and the
/// lightweight marker; clients only report what the current page exposed.
pub struct SaveLinkInput {
    pub url: String,
    pub canonical_url: String,
    pub title: String,
    pub author: Option<String>,
    pub site: Option<String>,
    pub saved_at: String,
    pub images: Vec<ImageBytes>,
    pub preview_url: Option<String>,
    pub favicon_url: Option<String>,
    pub theme_color: Option<String>,
    pub excerpt: Option<String>,
    pub lang: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveDisposition {
    Saved,
    Upgraded,
    Duplicate,
}

/// Structured, friendly result returned for both writes and duplicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    pub disposition: SaveDisposition,
    pub id: String,
    /// Path to `article.md`, relative to the library root.
    pub path: String,
}

/// Invalid capture data is kept distinct from storage failures so the native
/// messaging adapter can preserve its protocol-v3 error classification.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error(transparent)]
    Storage(#[from] anyhow::Error),
}

impl From<std::io::Error> for SaveError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.into())
    }
}

/// Save one capture, returning a duplicate as data rather than an error.
///
/// A full article capture is allowed to overwrite an earlier lightweight link
/// placeholder at the same URL-derived id. That upgrade preserves every field
/// controlled by the user after import; captured page metadata and content are
/// replaced by the richer browser result.
pub fn save_capture(library: &LibraryRoot, input: SaveInput) -> Result<SaveOutcome, SaveError> {
    save_with_imported_state(
        library,
        input,
        ImportedReadingState::default(),
        ImageWritePolicy::BestEffort,
        ExistingReadingPolicy::PreserveDuplicate,
    )
}

/// Save a fully captured source article and its pre-staged required assets.
///
/// Adapters resolve/fetch outside the core, write temporary local files, and
/// pass their declared content addresses here. The textual post belongs in
/// `capture.markdown`; ordered media presentation belongs in
/// `source_profile.attachments`.
pub fn save_source_capture(
    library: &LibraryRoot,
    input: SourceCaptureInput,
) -> Result<SaveOutcome, SaveError> {
    let SourceCaptureInput {
        capture,
        source_profile,
        staged_assets,
        expected_article,
    } = input;
    if capture.kind != ReadingKind::Article || capture.lightweight {
        return Err(SaveError::InvalidRequest(
            "source capture requires a full article SaveInput".to_string(),
        ));
    }
    validate_source_profile(&source_profile, &staged_assets)?;
    validate_expected_article_state(&expected_article)?;
    save_with_source_profile_and_assets(
        library,
        capture,
        ImportedReadingState::default(),
        ImageWritePolicy::Required,
        ExistingReadingPolicy::ReplaceArticleWithSourceProfile { expected_article },
        Some(source_profile),
        staged_assets,
    )
}

/// Import a fully described reading with initial user-controlled state.
///
/// A missing or whitespace-only `saved_at` falls back to the current UTC time.
/// This leniency is exclusive to migrations; browser captures continue to pass
/// through [`save_capture`] and retain their required timestamp unchanged.
pub fn import_reading(
    library: &LibraryRoot,
    mut input: SaveInput,
    state: ImportedReadingState,
) -> Result<SaveOutcome, SaveError> {
    if input.saved_at.trim().is_empty() {
        input.saved_at = crate::time::now_utc_iso();
    }
    save_with_imported_state(
        library,
        input,
        state,
        ImageWritePolicy::Required,
        ExistingReadingPolicy::PreserveDuplicate,
    )
}

#[derive(Clone, Copy)]
enum ImageWritePolicy {
    BestEffort,
    Required,
}

#[derive(Clone, PartialEq, Eq)]
enum ExistingReadingPolicy {
    PreserveDuplicate,
    ReplaceArticleWithSourceProfile {
        expected_article: ExpectedArticleState,
    },
    EnrichLightweightLink {
        expected_article_sha256: Option<String>,
    },
}

struct ExistingReadingSnapshot {
    reading: Reading,
    article_sha256: String,
}

struct CaptureSavePlan<'a> {
    imported_state: ImportedReadingState,
    image_write_policy: ImageWritePolicy,
    existing_policy: ExistingReadingPolicy,
    source_profile: Option<SourceProfile>,
    staged_assets: &'a mut [StagedSourceAsset],
}

fn save_with_imported_state(
    library: &LibraryRoot,
    input: SaveInput,
    state: ImportedReadingState,
    image_write_policy: ImageWritePolicy,
    existing_policy: ExistingReadingPolicy,
) -> Result<SaveOutcome, SaveError> {
    save_with_source_profile_and_assets(
        library,
        input,
        state,
        image_write_policy,
        existing_policy,
        None,
        Vec::new(),
    )
}

fn save_with_source_profile_and_assets(
    library: &LibraryRoot,
    input: SaveInput,
    state: ImportedReadingState,
    image_write_policy: ImageWritePolicy,
    existing_policy: ExistingReadingPolicy,
    source_profile: Option<SourceProfile>,
    mut staged_assets: Vec<StagedSourceAsset>,
) -> Result<SaveOutcome, SaveError> {
    let id = capture_id(&input)?;
    // Identity is known before touching disk. Hold its cross-process lock from
    // the duplicate/placeholder read through asset writes and atomic rename.
    let lock = lock_reading(library, &id)?;
    save_capture_under_lock_with_state(
        library,
        input,
        id,
        CaptureSavePlan {
            imported_state: state,
            image_write_policy,
            existing_policy,
            source_profile,
            staged_assets: &mut staged_assets,
        },
        &lock,
    )
}

#[cfg(test)]
fn save_capture_under_lock(
    library: &LibraryRoot,
    input: SaveInput,
    id: String,
    lock: &ReadingLock,
) -> Result<SaveOutcome, SaveError> {
    let mut staged_assets = Vec::new();
    save_capture_under_lock_with_state(
        library,
        input,
        id,
        CaptureSavePlan {
            imported_state: ImportedReadingState::default(),
            image_write_policy: ImageWritePolicy::BestEffort,
            existing_policy: ExistingReadingPolicy::PreserveDuplicate,
            source_profile: None,
            staged_assets: &mut staged_assets,
        },
        lock,
    )
}

fn save_capture_under_lock_with_state(
    library: &LibraryRoot,
    input: SaveInput,
    id: String,
    plan: CaptureSavePlan<'_>,
    lock: &ReadingLock,
) -> Result<SaveOutcome, SaveError> {
    let CaptureSavePlan {
        imported_state,
        image_write_policy,
        existing_policy,
        source_profile,
        staged_assets,
    } = plan;
    lock.ensure_protects(library, &id)?;
    let existing_id = find_existing(library, &input, &id)?;

    let previous = existing_id.as_deref().and_then(|existing| {
        let bytes = fs::read(library.article_path(existing)).ok()?;
        let article_sha256 = sha256_hex(&bytes);
        let content = std::str::from_utf8(&bytes).ok()?;
        let reading = parse_reading(content).ok()?;
        Some(ExistingReadingSnapshot {
            reading,
            article_sha256,
        })
    });
    let upgrading = previous.as_ref().is_some_and(|metadata| {
        input.kind == ReadingKind::Article
            && !input.lightweight
            && metadata.reading.metadata.kind == ReadingKind::Article
            && metadata.reading.metadata.lightweight
            && metadata.reading.metadata.id == id
    });
    let expected_article_sha256 = match &existing_policy {
        ExistingReadingPolicy::EnrichLightweightLink {
            expected_article_sha256,
        } => expected_article_sha256.as_deref(),
        ExistingReadingPolicy::PreserveDuplicate
        | ExistingReadingPolicy::ReplaceArticleWithSourceProfile { .. } => None,
    };
    let expected_source_article = match &existing_policy {
        ExistingReadingPolicy::ReplaceArticleWithSourceProfile { expected_article } => {
            Some(expected_article)
        }
        ExistingReadingPolicy::PreserveDuplicate
        | ExistingReadingPolicy::EnrichLightweightLink { .. } => None,
    };
    let source_expectation_matches = match expected_source_article {
        Some(expected) => expected_article_state_matches(library, &id, expected)?,
        None => true,
    };
    let allows_enrichment = matches!(
        &existing_policy,
        ExistingReadingPolicy::EnrichLightweightLink { .. }
    );
    let enriching = allows_enrichment
        && previous.as_ref().is_some_and(|snapshot| {
            input.kind == ReadingKind::Article
                && input.lightweight
                && snapshot.reading.metadata.kind == ReadingKind::Article
                && snapshot.reading.metadata.lightweight
                && snapshot.reading.metadata.id == id
                && url_id(&snapshot.reading.metadata.url).is_ok_and(|stored_id| stored_id == id)
                && expected_article_sha256
                    .is_none_or(|expected| snapshot.article_sha256 == expected)
        });
    let replacing_source = matches!(
        &existing_policy,
        ExistingReadingPolicy::ReplaceArticleWithSourceProfile { .. }
    ) && previous.as_ref().is_some_and(|snapshot| {
        let Some(incoming_profile) = source_profile.as_ref() else {
            return false;
        };
        input.kind == ReadingKind::Article
            && !input.lightweight
            && snapshot.reading.metadata.kind == ReadingKind::Article
            && snapshot.reading.metadata.id == id
            && url_id(&snapshot.reading.metadata.url).is_ok_and(|stored_id| stored_id == id)
            && snapshot
                .reading
                .metadata
                .source_profile
                .as_ref()
                .is_none_or(|existing_profile| {
                    existing_profile
                        .provider
                        .eq_ignore_ascii_case(&incoming_profile.provider)
                        && existing_profile.source_id == incoming_profile.source_id
                })
    });

    // Source resolution may perform minutes of network work. Treat the article
    // state observed before retrieval as a compare-and-swap precondition so a
    // newer local or synced article is never replaced by the stale result.
    if !source_expectation_matches {
        return Ok(outcome(
            library,
            SaveDisposition::Duplicate,
            existing_id.clone().unwrap_or_else(|| id.clone()),
        ));
    }

    // A conditional migration enriches an existing snapshot only. If the file
    // disappeared, stopped being a lightweight URL-derived link, or changed by
    // even one byte while metadata was fetched, report it as unchanged without
    // writing downloaded assets or recreating a missing reading.
    if expected_article_sha256.is_some() && !enriching {
        return Ok(outcome(
            library,
            SaveDisposition::Duplicate,
            existing_id.clone().unwrap_or_else(|| id.clone()),
        ));
    }

    if let Some(existing_id) = existing_id {
        if !upgrading && !enriching && !replacing_source {
            return Ok(outcome(library, SaveDisposition::Duplicate, existing_id));
        }
    }

    // Imported state is strictly an initializer for a new reading. An upgrade
    // keeps every user-controlled field already stored in Óia, and a
    // duplicate returned above performs no validation or writes at all.
    let imported_state = if upgrading || enriching || replacing_source {
        ImportedReadingState::default()
    } else {
        validate_imported_state(imported_state)?
    };

    let mut markdown = match image_write_policy {
        ImageWritePolicy::BestEffort => {
            write_images_under_lock(library, &id, &input.markdown, &input.images, lock)?
        }
        ImageWritePolicy::Required => {
            write_images_required_under_lock(library, &id, &input.markdown, &input.images, lock)?
        }
    };
    if enriching {
        if let Some(previous) = previous.as_ref() {
            markdown.clone_from(&previous.reading.body);
        }
    }
    let preview_asset = input
        .preview_url
        .as_deref()
        .and_then(|url| written_image_asset(library, &id, &input.images, url))
        .or_else(|| first_local_image_asset(&markdown));
    let favicon_asset = input
        .favicon_url
        .as_deref()
        .and_then(|url| written_image_asset(library, &id, &input.images, url));

    for asset in staged_assets {
        asset.persist(library, &id, lock)?;
    }

    let mut metadata = Metadata {
        format_version: 1,
        id: id.clone(),
        kind: input.kind,
        lightweight: input.lightweight,
        url: input.url,
        media_url: input.media_url,
        preview_asset,
        favicon_asset,
        theme_color: input.theme_color.as_deref().and_then(normalize_theme_color),
        canonical_url: input.canonical_url,
        title: input.title,
        author: input.author,
        site: input.site,
        source_profile,
        saved_at: input.saved_at,
        read_at: None,
        archived: false,
        favorite: imported_state.favorite,
        rating: 0,
        tags: imported_state.tags,
        machine_tags: vec![],
        excluded_machine_tags: vec![],
        excerpt: input.excerpt,
        word_count: input.word_count,
        lang: input.lang,
        source_hash: String::new(),
    };

    if let Some(previous) = previous.filter(|_| upgrading || enriching || replacing_source) {
        let previous = previous.reading.metadata;
        metadata.saved_at = previous.saved_at;
        metadata.read_at = previous.read_at;
        metadata.archived = previous.archived;
        metadata.favorite = previous.favorite;
        metadata.rating = previous.rating;
        metadata.tags = previous.tags;
        metadata.machine_tags = previous.machine_tags;
        metadata.excluded_machine_tags = previous.excluded_machine_tags;
        if metadata.preview_asset.is_none() {
            metadata.preview_asset = previous.preview_asset;
        }
        if metadata.favicon_asset.is_none() {
            metadata.favicon_asset = previous.favicon_asset;
        }
        if metadata.theme_color.is_none() {
            metadata.theme_color = previous.theme_color;
        }
        if metadata.author.is_none() {
            metadata.author = previous.author;
        }
        if metadata.site.is_none() {
            metadata.site = previous.site;
        }
        if metadata.source_profile.is_none() {
            metadata.source_profile = previous.source_profile;
        }
        if metadata.excerpt.is_none() {
            metadata.excerpt = previous.excerpt;
        }
        if metadata.word_count.is_none() {
            metadata.word_count = previous.word_count;
        }
        if metadata.lang.is_none() {
            metadata.lang = previous.lang;
        }
    }

    if !upgrading && !enriching && !replacing_source {
        if let Some(note_markdown) = imported_state.note_markdown.as_deref() {
            // The article is the commit marker for a reading. Store an explicit
            // imported note first so a successful article commit includes it.
            // None is intentionally non-destructive: an external sync process
            // may already have placed a pre-article sidecar in this folder.
            set_note_under_lock(library, &id, note_markdown, lock)?;
        }
    }

    // Asset adoption can take long enough for a non-cooperating sync writer to
    // change `article.md` despite Óia's reading lock. Re-check immediately
    // before the article rename; content-addressed assets left by a failed CAS
    // are harmless, while the newer article remains authoritative.
    if let Some(expected) = expected_source_article {
        if !expected_article_state_matches(library, &id, expected)? {
            return Ok(outcome(library, SaveDisposition::Duplicate, id));
        }
    }

    write_reading_under_lock(library, metadata, markdown, lock)?;
    Ok(outcome(
        library,
        if upgrading || enriching || replacing_source {
            SaveDisposition::Upgraded
        } else {
            SaveDisposition::Saved
        },
        id,
    ))
}

/// Import an HTTP(S) URL without fetching it. The placeholder deliberately uses
/// the normal article id so a later browser capture upgrades this same reading.
pub fn import_link(library: &LibraryRoot, url: &str) -> Result<SaveOutcome, SaveError> {
    import_link_with_options(library, url, ImportOptions::default())
}

/// Save an HTTP(S) link with metadata/assets captured from the live browser DOM.
/// It deliberately remains lightweight because no cleaned article body was
/// captured; a later full article save upgrades this same URL-derived reading.
pub fn save_link_capture(
    library: &LibraryRoot,
    input: SaveLinkInput,
) -> Result<SaveOutcome, SaveError> {
    let normalized = normalize_http_url(&input.url)?;
    let canonical_url =
        normalize_http_url(&input.canonical_url).unwrap_or_else(|_| normalized.clone());
    let parsed = Url::parse(&normalized).map_err(invalid)?;
    let site = input.site.or_else(|| parsed.host_str().map(str::to_string));

    save_capture(
        library,
        SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Article,
            lightweight: true,
            url: normalized.clone(),
            media_url: None,
            canonical_url,
            title: import_title(Some(&input.title), &normalized),
            author: input.author,
            site,
            saved_at: imported_saved_at(Some(input.saved_at)),
            markdown: format!("[Open link](<{normalized}>)"),
            images: input.images,
            preview_url: input.preview_url,
            favicon_url: input.favicon_url,
            theme_color: input.theme_color,
            excerpt: input.excerpt,
            word_count: None,
            lang: input.lang,
        },
    )
}

/// Import a lightweight link captured by an explicit migration adapter.
///
/// Unlike an ordinary browser re-save, this may enrich an existing lightweight
/// link at the same URL-derived id. It preserves every user-controlled field
/// and note while replacing source metadata and filling local preview/favicon
/// assets. Full articles and non-article readings remain duplicates.
pub fn import_link_capture(
    library: &LibraryRoot,
    input: SaveLinkInput,
    state: ImportedReadingState,
) -> Result<SaveOutcome, SaveError> {
    import_link_capture_with_snapshot(library, input, state, None)
}

/// Enrich an existing lightweight link only if its complete `article.md`
/// still has the exact SHA-256 observed by the migration plan.
///
/// The hash comparison and enrichment happen under the same reading lock. A
/// changed or missing snapshot is returned as [`SaveDisposition::Duplicate`]
/// and no metadata or downloaded assets are written.
pub fn import_link_capture_if_unchanged(
    library: &LibraryRoot,
    input: SaveLinkInput,
    state: ImportedReadingState,
    expected_article_sha256: &str,
) -> Result<SaveOutcome, SaveError> {
    if expected_article_sha256.len() != 64
        || !expected_article_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SaveError::InvalidRequest(
            "expected article snapshot must be 64 lowercase hexadecimal characters".to_string(),
        ));
    }
    import_link_capture_with_snapshot(
        library,
        input,
        state,
        Some(expected_article_sha256.to_string()),
    )
}

fn import_link_capture_with_snapshot(
    library: &LibraryRoot,
    input: SaveLinkInput,
    state: ImportedReadingState,
    expected_article_sha256: Option<String>,
) -> Result<SaveOutcome, SaveError> {
    let normalized = normalize_http_url(&input.url)?;
    let canonical_url =
        normalize_http_url(&input.canonical_url).unwrap_or_else(|_| normalized.clone());

    save_with_imported_state(
        library,
        SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Article,
            lightweight: true,
            url: normalized.clone(),
            media_url: None,
            canonical_url,
            title: import_title(Some(&input.title), &normalized),
            author: input.author,
            site: input.site,
            saved_at: imported_saved_at(Some(input.saved_at)),
            markdown: format!("[Open link](<{normalized}>)"),
            images: input.images,
            preview_url: input.preview_url,
            favicon_url: input.favicon_url,
            theme_color: input.theme_color,
            excerpt: input.excerpt,
            word_count: None,
            lang: input.lang,
        },
        state,
        ImageWritePolicy::Required,
        ExistingReadingPolicy::EnrichLightweightLink {
            expected_article_sha256,
        },
    )
}

/// Import an HTTP(S) URL with source metadata and initial user state.
pub fn import_link_with_options(
    library: &LibraryRoot,
    url: &str,
    options: ImportOptions,
) -> Result<SaveOutcome, SaveError> {
    let normalized = normalize_http_url(url)?;
    let parsed = Url::parse(&normalized).map_err(invalid)?;
    let site = parsed.host_str().map(str::to_string);
    let ImportOptions {
        title,
        saved_at,
        state,
    } = options;

    import_reading(
        library,
        SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Article,
            lightweight: true,
            url: normalized.clone(),
            media_url: None,
            canonical_url: normalized.clone(),
            title: import_title(title.as_deref(), &normalized),
            author: None,
            site,
            saved_at: imported_saved_at(saved_at),
            markdown: format!("[Open link](<{normalized}>)"),
            images: vec![],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            excerpt: None,
            word_count: None,
            lang: None,
        },
        state,
    )
}

/// Import source-less plain text as a quote. Identity ignores whitespace-only
/// differences while the stored body retains the user's line structure.
pub fn import_text(
    library: &LibraryRoot,
    text: &str,
    title: Option<&str>,
) -> Result<SaveOutcome, SaveError> {
    import_text_with_options(
        library,
        text,
        ImportOptions {
            title: title.map(str::to_string),
            ..ImportOptions::default()
        },
    )
}

/// Import source-less text with source metadata and initial user state.
pub fn import_text_with_options(
    library: &LibraryRoot,
    text: &str,
    options: ImportOptions,
) -> Result<SaveOutcome, SaveError> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = text.trim();
    let identity_text = normalized_text(trimmed);
    if identity_text.is_empty() {
        return Err(SaveError::InvalidRequest(
            "text import requires non-empty text".to_string(),
        ));
    }

    let content_hash = sha256_hex(format!("quote\0{identity_text}").as_bytes());
    let origin = format!("{LOCAL_ORIGIN_SCHEME}/quote/{content_hash}");
    let ImportOptions {
        title,
        saved_at,
        state,
    } = options;
    let title = import_title(title.as_deref(), "Saved quote");
    let markdown = trimmed
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    import_reading(
        library,
        SaveInput {
            quote_identity_markdown: Some(identity_text.clone()),
            kind: ReadingKind::Quote,
            lightweight: false,
            url: origin.clone(),
            media_url: None,
            canonical_url: origin,
            title,
            author: None,
            site: None,
            saved_at: imported_saved_at(saved_at),
            markdown,
            images: vec![],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            excerpt: Some(truncate_chars(&identity_text, QUOTE_EXCERPT_CHARACTERS)),
            word_count: Some(identity_text.split_whitespace().count() as u32),
            lang: None,
        },
        state,
    )
}

/// Import source-less image bytes. The byte hash drives both the synthetic
/// origin and the content-addressed media id, so filenames and pasteboard
/// representations of the same bytes deduplicate.
pub fn import_image(
    library: &LibraryRoot,
    bytes: Vec<u8>,
    content_type: &str,
    title: &str,
) -> Result<SaveOutcome, SaveError> {
    import_image_with_options(
        library,
        bytes,
        content_type,
        ImportOptions {
            title: Some(title.to_string()),
            ..ImportOptions::default()
        },
    )
}

/// Import source-less image bytes with source metadata and initial user state.
pub fn import_image_with_options(
    library: &LibraryRoot,
    bytes: Vec<u8>,
    content_type: &str,
    options: ImportOptions,
) -> Result<SaveOutcome, SaveError> {
    if bytes.is_empty() {
        return Err(SaveError::InvalidRequest(
            "image import requires non-empty bytes".to_string(),
        ));
    }

    let content_hash = sha256_hex(&bytes);
    let origin = format!("{LOCAL_ORIGIN_SCHEME}/image/{content_hash}");
    let ImportOptions {
        title,
        saved_at,
        state,
    } = options;
    let title = import_title(title.as_deref(), "Imported image");

    import_reading(
        library,
        SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Image,
            lightweight: false,
            url: origin.clone(),
            media_url: Some(origin.clone()),
            canonical_url: origin.clone(),
            title,
            author: None,
            site: None,
            saved_at: imported_saved_at(saved_at),
            markdown: format!("![Imported image]({origin})"),
            images: vec![ImageBytes {
                url: origin,
                content_type: content_type.trim().to_string(),
                bytes,
            }],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            excerpt: None,
            word_count: None,
            lang: None,
        },
        state,
    )
}

/// Import image bytes captured from an HTTP(S) origin page.
///
/// The page remains the reading's origin while `media_url` is a stable local
/// reference derived from the copied bytes. This gives identical bytes from
/// different pages distinct reading ids without leaking an importer file path.
pub fn import_image_from_origin_with_options(
    library: &LibraryRoot,
    bytes: Vec<u8>,
    content_type: &str,
    origin_url: &str,
    options: ImportOptions,
) -> Result<SaveOutcome, SaveError> {
    if bytes.is_empty() {
        return Err(SaveError::InvalidRequest(
            "image import requires non-empty bytes".to_string(),
        ));
    }

    let origin = normalize_http_url(origin_url)?;
    let parsed = Url::parse(&origin).map_err(invalid)?;
    let content_hash = sha256_hex(&bytes);
    // The origin is a page URL, not an image filename. Unknown media types use
    // `.bin` rather than borrowing a potentially misleading page extension.
    let extension = image_extension(content_type, "");
    let relative_asset = format!("assets/{content_hash}.{extension}");
    let media_reference = format!("{LOCAL_ASSET_SCHEME}{relative_asset}");
    let ImportOptions {
        title,
        saved_at,
        state,
    } = options;

    import_reading(
        library,
        SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Image,
            lightweight: false,
            url: origin.clone(),
            media_url: Some(media_reference.clone()),
            canonical_url: origin,
            title: import_title(title.as_deref(), "Imported image"),
            author: None,
            site: parsed.host_str().map(str::to_string),
            saved_at: imported_saved_at(saved_at),
            markdown: format!("![Imported image]({media_reference})"),
            images: vec![ImageBytes {
                url: media_reference,
                content_type: content_type.trim().to_string(),
                bytes,
            }],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            excerpt: None,
            word_count: None,
            lang: None,
        },
        state,
    )
}

/// Begin a bounded, incremental import for a browser-saved video.
pub fn begin_browser_video_import(
    library: &LibraryRoot,
    mut input: BrowserVideoImportInput,
) -> Result<BrowserVideoImport, SaveError> {
    if let Some(expected_bytes) = input.expected_bytes {
        if expected_bytes == 0 {
            return Err(SaveError::InvalidRequest(
                "browser video import requires non-empty bytes".to_string(),
            ));
        }
        if expected_bytes > MAX_BROWSER_VIDEO_BYTES {
            return Err(SaveError::InvalidRequest(format!(
                "browser video exceeds the {} byte limit",
                MAX_BROWSER_VIDEO_BYTES
            )));
        }
    }

    input.origin_url = normalize_http_url(&input.origin_url)?;
    input.canonical_url =
        normalize_http_url(&input.canonical_url).unwrap_or_else(|_| input.origin_url.clone());
    let extension = video_extension(&input.content_type)?.to_string();
    let staging_dir = library.path().join(IMPORT_STAGING_DIRECTORY);
    fs::create_dir_all(&staging_dir)?;
    let staging_path = staging_dir.join(format!("video-{}.tmp", crate::new_id()));
    let destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging_path)?;

    Ok(BrowserVideoImport {
        library_path: library.path().to_path_buf(),
        input: Some(input),
        extension,
        staging_path: Some(staging_path),
        destination: Some(destination),
        hasher: Sha256::new(),
        byte_count: 0,
        failed: false,
    })
}

impl BrowserVideoImport {
    /// Append one decoded browser-message chunk without retaining it in memory.
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), SaveError> {
        if self.failed {
            return Err(SaveError::InvalidRequest(
                "browser video import cannot continue after an earlier failure".to_string(),
            ));
        }

        let next_count = self
            .byte_count
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| {
                self.failed = true;
                SaveError::InvalidRequest("browser video byte count overflowed".to_string())
            })?;
        if next_count > MAX_BROWSER_VIDEO_BYTES {
            self.failed = true;
            return Err(SaveError::InvalidRequest(format!(
                "browser video exceeds the {} byte limit",
                MAX_BROWSER_VIDEO_BYTES
            )));
        }
        if self
            .input
            .as_ref()
            .and_then(|input| input.expected_bytes)
            .is_some_and(|expected| next_count > expected)
        {
            self.failed = true;
            return Err(SaveError::InvalidRequest(
                "browser video exceeded its declared byte count".to_string(),
            ));
        }

        let destination = self.destination.as_mut().ok_or_else(|| {
            self.failed = true;
            SaveError::InvalidRequest("browser video import is already finished".to_string())
        })?;
        if let Err(error) = destination.write_all(bytes) {
            self.failed = true;
            return Err(error.into());
        }
        self.hasher.update(bytes);
        self.byte_count = next_count;
        Ok(())
    }

    /// Commit the completed stream as a content-addressed local video asset.
    pub fn finish(mut self) -> Result<SaveOutcome, SaveError> {
        if self.failed {
            return Err(SaveError::InvalidRequest(
                "browser video import cannot finish after an earlier failure".to_string(),
            ));
        }
        if self.byte_count == 0 {
            return Err(SaveError::InvalidRequest(
                "browser video import requires non-empty bytes".to_string(),
            ));
        }

        let input = self
            .input
            .take()
            .expect("browser video import input exists until finish");
        if input
            .expected_bytes
            .is_some_and(|expected| expected != self.byte_count)
        {
            return Err(SaveError::InvalidRequest(format!(
                "browser video byte count did not match the declared size: expected {}, received {}",
                input.expected_bytes.unwrap(),
                self.byte_count
            )));
        }

        let destination = self
            .destination
            .take()
            .expect("browser video staging file exists until finish");
        destination.sync_all()?;
        drop(destination);
        validate_browser_video_container(
            self.staging_path
                .as_deref()
                .expect("browser video staging path exists until finish"),
            &self.extension,
        )?;

        let staged = StagedVideo {
            path: self.staging_path.take(),
            content_hash: hex::encode(self.hasher.clone().finalize()),
        };
        let library = LibraryRoot::new(&self.library_path)?;
        let origin = input.origin_url;
        let fallback_site = Url::parse(&origin)
            .map_err(invalid)?
            .host_str()
            .map(str::to_string);
        let site = input
            .site
            .filter(|site| !site.trim().is_empty())
            .or(fallback_site);
        let extension = std::mem::take(&mut self.extension);

        save_staged_video(
            &library,
            staged,
            &extension,
            ImportOptions {
                title: Some(input.title),
                saved_at: Some(input.saved_at),
                ..ImportOptions::default()
            },
            move |_content_hash, media_reference| {
                let id =
                    media_id(ReadingKind::Video, &origin, media_reference).map_err(|error| {
                        SaveError::InvalidRequest(format!("could not derive reading id: {error}"))
                    })?;
                Ok(VideoImportIdentity {
                    id,
                    origin,
                    canonical_url: input.canonical_url,
                    author: input.author,
                    site,
                    theme_color: input.theme_color,
                    lang: input.lang,
                    excerpt: input.excerpt,
                    word_count: input.word_count,
                })
            },
        )
    }
}

impl Drop for BrowserVideoImport {
    fn drop(&mut self) {
        self.destination.take();
        if let Some(path) = self.staging_path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

/// Import a source-less local video without materializing it in memory.
///
/// The source is streamed once into a temporary file inside the library while
/// its bytes are hashed. The content hash determines the reading id and final
/// asset filename, so filenames and MIME aliases never affect deduplication.
/// The caller's path is used only for this copy and is never persisted.
pub fn import_video_file(
    library: &LibraryRoot,
    file_path: &Path,
    content_type: &str,
    title: &str,
) -> Result<SaveOutcome, SaveError> {
    import_video_file_with_options(
        library,
        file_path,
        content_type,
        ImportOptions {
            title: Some(title.to_string()),
            ..ImportOptions::default()
        },
    )
}

/// Import a source-less local video with source metadata and initial user
/// state, without materializing the video in memory.
pub fn import_video_file_with_options(
    library: &LibraryRoot,
    file_path: &Path,
    content_type: &str,
    options: ImportOptions,
) -> Result<SaveOutcome, SaveError> {
    let extension = video_extension(content_type)?;
    let staged = stage_video(library, file_path)?;
    save_staged_video(
        library,
        staged,
        extension,
        options,
        |content_hash, _media_reference| {
            Ok(VideoImportIdentity::with_default_metadata(
                local_video_id(content_hash),
                format!("{LOCAL_ORIGIN_SCHEME}/video/{content_hash}"),
                None,
            ))
        },
    )
}

/// Import a local video copied from an HTTP(S) origin page.
///
/// The copied asset supplies the media identity and playback reference, while
/// the normalized page URL remains the origin. The caller's source path is used
/// only for the streamed copy and is never persisted.
pub fn import_video_file_from_origin_with_options(
    library: &LibraryRoot,
    file_path: &Path,
    content_type: &str,
    origin_url: &str,
    options: ImportOptions,
) -> Result<SaveOutcome, SaveError> {
    let origin = normalize_http_url(origin_url)?;
    let parsed = Url::parse(&origin).map_err(invalid)?;
    let site = parsed.host_str().map(str::to_string);
    let extension = video_extension(content_type)?;
    let staged = stage_video(library, file_path)?;
    save_staged_video(
        library,
        staged,
        extension,
        options,
        move |_content_hash, media_reference| {
            let id = media_id(ReadingKind::Video, &origin, media_reference).map_err(|error| {
                SaveError::InvalidRequest(format!("could not derive reading id: {error}"))
            })?;
            Ok(VideoImportIdentity::with_default_metadata(id, origin, site))
        },
    )
}

struct VideoImportIdentity {
    id: String,
    origin: String,
    canonical_url: String,
    author: Option<String>,
    site: Option<String>,
    theme_color: Option<String>,
    lang: Option<String>,
    excerpt: Option<String>,
    word_count: Option<u32>,
}

impl VideoImportIdentity {
    fn with_default_metadata(id: String, origin: String, site: Option<String>) -> Self {
        Self {
            id,
            canonical_url: origin.clone(),
            origin,
            author: None,
            site,
            theme_color: None,
            lang: None,
            excerpt: None,
            word_count: None,
        }
    }
}

fn save_staged_video(
    library: &LibraryRoot,
    mut staged: StagedVideo,
    extension: &str,
    options: ImportOptions,
    identity: impl FnOnce(&str, &str) -> Result<VideoImportIdentity, SaveError>,
) -> Result<SaveOutcome, SaveError> {
    let content_hash = staged.content_hash.clone();
    let filename = format!("{content_hash}.{extension}");
    let relative_asset = format!("assets/{filename}");
    let media_reference = format!("{LOCAL_ASSET_SCHEME}{relative_asset}");
    let VideoImportIdentity {
        id,
        origin,
        canonical_url,
        author,
        site,
        theme_color,
        lang,
        excerpt,
        word_count,
    } = identity(&content_hash, &media_reference)?;

    // Hashing must finish before the content-addressed lock is known. Hold it
    // from the duplicate check through the asset move and article rename so two
    // processes importing the same bytes cannot race each other.
    let lock = lock_reading(library, &id)?;
    if library.article_path(&id).is_file() {
        restore_missing_video_asset(library, &id, &content_hash, &mut staged)?;
        return Ok(outcome(library, SaveDisposition::Duplicate, id));
    }

    let ImportOptions {
        title,
        saved_at,
        state,
    } = options;
    let imported_state = validate_imported_state(state)?;

    let assets_dir = library.assets_dir(&id);
    fs::create_dir_all(&assets_dir)?;
    let asset_path = assets_dir.join(&filename);
    staged.persist(&asset_path)?;

    let metadata = Metadata {
        format_version: 1,
        id: id.clone(),
        kind: ReadingKind::Video,
        lightweight: false,
        url: origin.clone(),
        media_url: Some(media_reference),
        preview_asset: None,
        favicon_asset: None,
        theme_color: theme_color.as_deref().and_then(normalize_theme_color),
        canonical_url,
        title: import_title(title.as_deref(), "Imported video"),
        author,
        site,
        source_profile: None,
        saved_at: imported_saved_at(saved_at),
        read_at: None,
        archived: false,
        favorite: imported_state.favorite,
        rating: 0,
        tags: imported_state.tags,
        machine_tags: vec![],
        excluded_machine_tags: vec![],
        excerpt,
        word_count,
        lang,
        source_hash: String::new(),
    };
    let body = format!("[Play video]({relative_asset})");

    if let Some(note_markdown) = imported_state.note_markdown.as_deref() {
        set_note_under_lock(library, &id, note_markdown, &lock)?;
    }

    // The final content-addressed asset may have arrived from external sync;
    // never remove it when a later note or article commit fails. Without
    // article.md the orphan stays invisible and a retry can safely reuse it.
    write_reading_under_lock(library, metadata, body, &lock)?;

    Ok(outcome(library, SaveDisposition::Saved, id))
}

fn restore_missing_video_asset(
    library: &LibraryRoot,
    id: &str,
    content_hash: &str,
    staged: &mut StagedVideo,
) -> Result<(), SaveError> {
    let metadata = read_metadata(&library.article_path(id))?;
    if metadata.kind != ReadingKind::Video {
        return Err(existing_video_error(
            id,
            "the content-addressed reading is not a video",
        ));
    }

    let media_reference = metadata.media_url.as_deref().ok_or_else(|| {
        existing_video_error(id, "the existing video has no local media reference")
    })?;
    let relative_asset = media_reference
        .strip_prefix(LOCAL_ASSET_SCHEME)
        .and_then(|relative| relative.strip_prefix("assets/"))
        .ok_or_else(|| {
            existing_video_error(
                id,
                "the existing video media reference is not a local asset",
            )
        })?;
    if relative_asset.is_empty() || relative_asset.contains('/') {
        return Err(existing_video_error(
            id,
            "the existing video asset path is not safe",
        ));
    }

    let (stored_hash, stored_extension) = relative_asset.split_once('.').ok_or_else(|| {
        existing_video_error(id, "the existing video asset name is not content-addressed")
    })?;
    if stored_hash.len() != 64
        || !stored_hash
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        || !stored_hash.eq_ignore_ascii_case(content_hash)
        || stored_extension.contains('.')
        || !is_safe_video_extension(stored_extension)
    {
        return Err(existing_video_error(
            id,
            "the existing video asset name does not match the imported content",
        ));
    }

    let asset_path = library.assets_dir(id).join(relative_asset);
    if asset_path.is_file() {
        return Ok(());
    }
    if asset_path.exists() {
        return Err(existing_video_error(
            id,
            "the existing video asset path is not a regular file",
        ));
    }

    fs::create_dir_all(library.assets_dir(id))?;
    staged.persist(&asset_path)
}

fn existing_video_error(id: &str, reason: &str) -> SaveError {
    SaveError::Storage(anyhow::anyhow!(
        "could not reconcile existing video reading {id}: {reason}"
    ))
}

fn import_title(title: Option<&str>, fallback: &str) -> String {
    title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn imported_saved_at(saved_at: Option<String>) -> String {
    saved_at
        .filter(|saved_at| !saved_at.trim().is_empty())
        .unwrap_or_else(crate::time::now_utc_iso)
}

fn validate_imported_state(
    mut state: ImportedReadingState,
) -> Result<ImportedReadingState, SaveError> {
    let mut tags = Vec::with_capacity(state.tags.len());
    for raw_tag in state.tags {
        let tag = validate_imported_tag(&raw_tag).map_err(|error| {
            SaveError::InvalidRequest(format!("invalid imported tag {raw_tag:?}: {error}"))
        })?;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    state.tags = tags;
    if state
        .note_markdown
        .as_ref()
        .is_some_and(|markdown| markdown.trim().is_empty())
    {
        state.note_markdown = None;
    }
    Ok(state)
}

/// A source-less video's id depends only on its kind and raw content hash. The
/// normalized extension is intentionally absent so the same bytes deduplicate
/// even if a caller supplied another filename or equivalent content type.
fn local_video_id(content_hash: &str) -> String {
    sha256_hex(format!("video\0{content_hash}").as_bytes())
}

fn video_extension(content_type: &str) -> Result<&'static str, SaveError> {
    let normalized = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "video/mp4" => Ok("mp4"),
        "video/quicktime" => Ok("mov"),
        "video/m4v" | "video/x-m4v" => Ok("m4v"),
        "video/webm" => Ok("webm"),
        "video/mpeg" => Ok("mpeg"),
        "video/avi" | "video/x-msvideo" => Ok("avi"),
        "video/ogg" => Ok("ogv"),
        "video/x-matroska" => Ok("mkv"),
        "video/3gpp" => Ok("3gp"),
        "video/3gpp2" => Ok("3g2"),
        _ => Err(SaveError::InvalidRequest(format!(
            "unsupported video content type: {}",
            content_type.trim()
        ))),
    }
}

fn is_safe_video_extension(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp4" | "mov" | "m4v" | "webm" | "mpeg" | "avi" | "ogv" | "mkv" | "3gp" | "3g2"
    )
}

fn validate_browser_video_container(path: &Path, extension: &str) -> Result<(), SaveError> {
    const SIGNATURE_BYTES: usize = 64 * 1024;

    let mut file = fs::File::open(path)?;
    let mut prefix = vec![0_u8; SIGNATURE_BYTES];
    let read = file.read(&mut prefix)?;
    prefix.truncate(read);

    let valid = match extension {
        "mp4" | "mov" | "m4v" | "3gp" | "3g2" => is_iso_base_media(&prefix),
        "webm" => ebml_document_type(&prefix) == Some(b"webm".as_slice()),
        "mkv" => ebml_document_type(&prefix) == Some(b"matroska".as_slice()),
        "mpeg" => {
            prefix.starts_with(&[0x00, 0x00, 0x01, 0xba])
                || prefix.starts_with(&[0x00, 0x00, 0x01, 0xb3])
        }
        "avi" => prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"AVI ",
        "ogv" => prefix.starts_with(b"OggS"),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(SaveError::InvalidRequest(format!(
            "browser video does not contain a valid video container for .{extension}"
        )))
    }
}

fn is_iso_base_media(bytes: &[u8]) -> bool {
    let mut offset = 0_usize;

    // A file-type box is normally first, but ISO BMFF and QuickTime also allow
    // harmless padding boxes before it. Parse their declared sizes instead of
    // accepting an arbitrary `ftyp` byte sequence elsewhere in the payload.
    for _ in 0..4 {
        if bytes.len().saturating_sub(offset) < 8 {
            return false;
        }
        let size32 = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as u64;
        let box_type = &bytes[offset + 4..offset + 8];
        let (box_size, header_size) = if size32 == 1 {
            if bytes.len().saturating_sub(offset) < 16 {
                return false;
            }
            (
                u64::from_be_bytes(bytes[offset + 8..offset + 16].try_into().unwrap()),
                16_usize,
            )
        } else {
            (size32, 8_usize)
        };
        let Ok(box_size) = usize::try_from(box_size) else {
            return false;
        };
        if box_size < header_size || box_size > bytes.len().saturating_sub(offset) {
            return false;
        }

        if box_type == b"ftyp" {
            let payload = &bytes[offset + header_size..offset + box_size];
            return payload.len() >= 8
                && payload.len().is_multiple_of(4)
                && payload[..4]
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b' ')
                && payload[..4].iter().any(|byte| *byte != b' ');
        }
        if !matches!(box_type, b"free" | b"skip" | b"wide") || box_size == 0 {
            return false;
        }
        offset += box_size;
    }
    false
}

fn ebml_document_type(bytes: &[u8]) -> Option<&[u8]> {
    if !bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return None;
    }
    let (header_size, size_width) = ebml_size(&bytes[4..])?;
    let mut offset = 4 + size_width;
    let header_end = offset.checked_add(header_size)?;
    if header_end > bytes.len() {
        return None;
    }

    while offset < header_end {
        let id_width = bytes[offset].leading_zeros() as usize + 1;
        if !(1..=4).contains(&id_width) || offset.checked_add(id_width)? > header_end {
            return None;
        }
        let id = &bytes[offset..offset + id_width];
        offset += id_width;
        let (payload_size, size_width) = ebml_size(&bytes[offset..header_end])?;
        offset += size_width;
        let payload_end = offset.checked_add(payload_size)?;
        if payload_end > header_end {
            return None;
        }
        if id == [0x42, 0x82] {
            return Some(&bytes[offset..payload_end]);
        }
        offset = payload_end;
    }
    None
}

fn ebml_size(bytes: &[u8]) -> Option<(usize, usize)> {
    let first = *bytes.first()?;
    let width = first.leading_zeros() as usize + 1;
    if width > 8 || bytes.len() < width {
        return None;
    }
    let marker = 1_u8 << (8 - width);
    let mut value = usize::from(first & (marker - 1));
    for byte in &bytes[1..width] {
        value = value.checked_mul(256)?.checked_add(usize::from(*byte))?;
    }
    Some((value, width))
}

fn validate_staged_asset_declaration(
    asset: &str,
    sha256: &str,
    byte_count: u64,
) -> Result<(), SaveError> {
    if byte_count == 0 {
        return Err(SaveError::InvalidRequest(
            "staged source assets must be non-empty".to_string(),
        ));
    }
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SaveError::InvalidRequest(
            "staged source asset sha256 must be 64 lowercase hexadecimal characters".to_string(),
        ));
    }
    let filename = asset.strip_prefix("assets/").filter(|filename| {
        !filename.is_empty()
            && !filename.contains('/')
            && !filename.contains('\\')
            && *filename != "."
            && *filename != ".."
    });
    let Some(filename) = filename else {
        return Err(SaveError::InvalidRequest(
            "staged source asset target must be assets/<file>".to_string(),
        ));
    };
    let Some((stem, extension)) = filename.rsplit_once('.') else {
        return Err(SaveError::InvalidRequest(
            "staged source asset target requires a file extension".to_string(),
        ));
    };
    if stem != sha256
        || extension.is_empty()
        || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(SaveError::InvalidRequest(
            "staged source asset target must be assets/<sha256>.<ext>".to_string(),
        ));
    }
    Ok(())
}

fn validate_source_profile(
    profile: &SourceProfile,
    staged_assets: &[StagedSourceAsset],
) -> Result<(), SaveError> {
    if profile.version == 0
        || profile.source_type.trim().is_empty()
        || profile.provider.trim().is_empty()
        || profile.source_id.trim().is_empty()
        || profile.author_handle.trim().is_empty()
    {
        return Err(SaveError::InvalidRequest(
            "source profile requires a positive version and non-empty source/provider/author fields"
                .to_string(),
        ));
    }

    let mut declared = HashSet::new();
    for staged in staged_assets {
        if !declared.insert(staged.asset()) {
            return Err(SaveError::InvalidRequest(format!(
                "source capture declares duplicate staged asset {}",
                staged.asset()
            )));
        }
    }

    let required = profile
        .avatar_asset
        .iter()
        .chain(profile.attachments.iter().flat_map(|attachment| {
            std::iter::once(&attachment.asset).chain(attachment.poster_asset.iter())
        }));
    for asset in required {
        if !declared.contains(asset.as_str()) {
            return Err(SaveError::InvalidRequest(format!(
                "source profile asset is missing a required staged file: {asset}"
            )));
        }
    }
    if profile
        .attachments
        .iter()
        .any(|attachment| attachment.kind.trim().is_empty())
    {
        return Err(SaveError::InvalidRequest(
            "source attachment kind must not be empty".to_string(),
        ));
    }
    if profile
        .attachments
        .iter()
        .any(|attachment| attachment.width == Some(0) || attachment.height == Some(0))
    {
        return Err(SaveError::InvalidRequest(
            "source attachment dimensions must be positive when present".to_string(),
        ));
    }
    Ok(())
}

fn validate_expected_article_state(expected: &ExpectedArticleState) -> Result<(), SaveError> {
    let ExpectedArticleState::Sha256(sha256) = expected else {
        return Ok(());
    };
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SaveError::InvalidRequest(
            "expected source article sha256 must be 64 lowercase hexadecimal characters"
                .to_string(),
        ));
    }
    Ok(())
}

fn expected_article_state_matches(
    library: &LibraryRoot,
    id: &str,
    expected: &ExpectedArticleState,
) -> Result<bool, SaveError> {
    match fs::read(library.article_path(id)) {
        Ok(bytes) => Ok(match expected {
            ExpectedArticleState::Missing => false,
            ExpectedArticleState::Sha256(expected) => {
                sha256_hex(&bytes).as_str() == expected.as_str()
            }
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(matches!(expected, ExpectedArticleState::Missing))
        }
        Err(error) => Err(error.into()),
    }
}

impl StagedSourceAsset {
    fn persist(
        &mut self,
        library: &LibraryRoot,
        id: &str,
        lock: &ReadingLock,
    ) -> Result<(), SaveError> {
        lock.ensure_protects(library, id)?;
        let source_path = self
            .staging_path
            .as_ref()
            .expect("staged source asset has not yet been committed")
            .clone();
        let source_metadata = fs::symlink_metadata(&source_path)?;
        if !source_metadata.file_type().is_file() {
            return Err(SaveError::InvalidRequest(
                "staged source asset must be a regular file".to_string(),
            ));
        }
        if source_metadata.len() != self.byte_count {
            return Err(SaveError::InvalidRequest(format!(
                "staged source asset byte count did not match: expected {}, received {}",
                self.byte_count,
                source_metadata.len()
            )));
        }

        let destination = library.reading_dir(id).join(&self.asset);
        if let (Ok(source), Ok(target)) = (
            fs::canonicalize(&source_path),
            fs::canonicalize(&destination),
        ) {
            if source == target {
                self.staging_path = None;
                return Err(SaveError::InvalidRequest(
                    "staged source path must be outside its final asset path".to_string(),
                ));
            }
        }

        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_file() => {
                if metadata.len() == self.byte_count {
                    let (hash, size) = hash_file(&destination)?;
                    if hash == self.sha256 && size == self.byte_count {
                        return self.remove_staging_file();
                    }
                }
                // A sync conflict or interrupted external write may have left
                // corrupt bytes at the content-addressed path. The freshly
                // downloaded, verified asset below atomically repairs it.
            }
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "source asset path is not a regular file: {}",
                        destination.display()
                    ),
                )
                .into())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let assets_dir = library.assets_dir(id);
        fs::create_dir_all(&assets_dir)?;
        let temp_path = assets_dir.join(format!(".asset.{}.tmp", crate::new_id()));
        let commit_result = (|| -> Result<(), SaveError> {
            let mut source = fs::File::open(&source_path)?;
            let mut temporary = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            let (hash, size) = stream_and_hash(&mut source, &mut temporary)?;
            temporary.sync_all()?;
            drop(temporary);
            if hash != self.sha256 || size != self.byte_count {
                return Err(SaveError::InvalidRequest(format!(
                    "staged source asset did not match its declared hash/size: {}",
                    self.asset
                )));
            }
            fs::rename(&temp_path, &destination)?;
            self.remove_staging_file()?;
            Ok(())
        })();
        if commit_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        commit_result
    }

    fn remove_staging_file(&mut self) -> Result<(), SaveError> {
        let path = self
            .staging_path
            .take()
            .expect("staged source asset still owns its temporary file");
        if let Err(error) = fs::remove_file(&path) {
            self.staging_path = Some(path);
            return Err(error.into());
        }
        Ok(())
    }
}

fn hash_file(path: &Path) -> Result<(String, u64), SaveError> {
    let mut source = fs::File::open(path)?;
    let mut sink = std::io::sink();
    stream_and_hash(&mut source, &mut sink).map_err(Into::into)
}

struct StagedVideo {
    path: Option<PathBuf>,
    content_hash: String,
}

impl StagedVideo {
    fn persist(&mut self, destination: &Path) -> Result<(), SaveError> {
        let source = self.path.as_ref().expect("staged video not yet persisted");
        fs::rename(source, destination)?;
        self.path = None;
        Ok(())
    }
}

impl Drop for StagedVideo {
    fn drop(&mut self) {
        if let Some(path) = self.path.as_ref() {
            let _ = fs::remove_file(path);
        }
    }
}

fn stage_video(library: &LibraryRoot, file_path: &Path) -> Result<StagedVideo, SaveError> {
    let metadata = fs::metadata(file_path).map_err(|error| {
        SaveError::InvalidRequest(format!("could not read video file: {error}"))
    })?;
    if !metadata.is_file() {
        return Err(SaveError::InvalidRequest(
            "video import source must be a regular file".to_string(),
        ));
    }

    let mut source = fs::File::open(file_path)?;
    let staging_dir = library.path().join(IMPORT_STAGING_DIRECTORY);
    fs::create_dir_all(&staging_dir)?;
    let staging_path = staging_dir.join(format!("video-{}.tmp", crate::new_id()));
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging_path)?;

    let copy_result = stream_and_hash(&mut source, &mut destination);
    let sync_result = copy_result.as_ref().ok().map(|_| destination.sync_all());
    drop(destination);

    let (content_hash, byte_count) = match copy_result {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_file(&staging_path);
            return Err(error.into());
        }
    };
    if let Some(Err(error)) = sync_result {
        let _ = fs::remove_file(&staging_path);
        return Err(error.into());
    }
    if byte_count == 0 {
        let _ = fs::remove_file(&staging_path);
        return Err(SaveError::InvalidRequest(
            "video import requires a non-empty file".to_string(),
        ));
    }

    Ok(StagedVideo {
        path: Some(staging_path),
        content_hash,
    })
}

fn stream_and_hash(
    source: &mut fs::File,
    destination: &mut impl Write,
) -> std::io::Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut byte_count = 0_u64;
    let mut buffer = vec![0_u8; VIDEO_COPY_BUFFER_SIZE];

    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        destination.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        byte_count += read as u64;
    }

    Ok((hex::encode(hasher.finalize()), byte_count))
}

fn capture_id(input: &SaveInput) -> Result<String, SaveError> {
    let result = match input.kind {
        ReadingKind::Article => url_id(&input.url),
        ReadingKind::Image | ReadingKind::Video => match input.media_url.as_deref() {
            Some(media_url) => media_id(input.kind, &input.url, media_url),
            None => {
                return Err(SaveError::InvalidRequest(
                    "image and video saves require metadata.media_url".to_string(),
                ))
            }
        },
        ReadingKind::Quote => quote_id(
            &input.url,
            input
                .quote_identity_markdown
                .as_deref()
                .unwrap_or(&input.markdown),
        ),
    };
    result
        .map_err(|error| SaveError::InvalidRequest(format!("could not derive reading id: {error}")))
}

fn find_existing(
    library: &LibraryRoot,
    input: &SaveInput,
    id: &str,
) -> Result<Option<String>, SaveError> {
    match input.kind {
        ReadingKind::Article => find_by_url(library, &input.url).map_err(Into::into),
        ReadingKind::Image | ReadingKind::Video => find_by_media(
            library,
            input.kind,
            &input.url,
            input.media_url.as_deref().expect("validated by capture_id"),
        )
        .map_err(Into::into),
        ReadingKind::Quote => Ok(library.article_path(id).is_file().then(|| id.to_string())),
    }
}

fn outcome(library: &LibraryRoot, disposition: SaveDisposition, id: String) -> SaveOutcome {
    let article_path = library.article_path(&id);
    let path = article_path
        .strip_prefix(library.path())
        .unwrap_or(&article_path)
        .to_string_lossy()
        .into_owned();
    SaveOutcome {
        disposition,
        id,
        path,
    }
}

fn normalize_http_url(raw: &str) -> Result<String, SaveError> {
    let parsed = Url::parse(raw.trim()).map_err(invalid)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SaveError::InvalidRequest(
            "link import requires an http(s) URL".to_string(),
        ));
    }
    crate::normalize_url(parsed.as_str()).map_err(SaveError::Storage)
}

/// Normalize the CSS color forms commonly used by `<meta name="theme-color">`
/// into the single portable representation stored by the library contract.
/// Unsupported or non-finite values are ignored rather than making an
/// otherwise useful capture fail.
pub fn normalize_theme_color(raw: &str) -> Option<String> {
    let value = raw.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        let expanded = match hex.len() {
            3 | 4 if hex.chars().all(|character| character.is_ascii_hexdigit()) => hex
                .chars()
                .take(3)
                .flat_map(|character| [character, character])
                .collect::<String>(),
            6 | 8 if hex.chars().all(|character| character.is_ascii_hexdigit()) => {
                hex[..6].to_string()
            }
            _ => return None,
        };
        return Some(format!("#{expanded}"));
    }

    let named = match value.as_str() {
        "black" => Some((0, 0, 0)),
        "white" => Some((255, 255, 255)),
        "red" => Some((255, 0, 0)),
        "green" => Some((0, 128, 0)),
        "blue" => Some((0, 0, 255)),
        "gray" | "grey" => Some((128, 128, 128)),
        "yellow" => Some((255, 255, 0)),
        "navy" => Some((0, 0, 128)),
        "teal" => Some((0, 128, 128)),
        _ => None,
    };
    if let Some((red, green, blue)) = named {
        return Some(rgb_hex(red, green, blue));
    }

    if let Some(arguments) =
        css_function_arguments(&value, "rgb").or_else(|| css_function_arguments(&value, "rgba"))
    {
        let tokens = css_color_tokens(arguments);
        if tokens.len() < 3 {
            return None;
        }
        return Some(rgb_hex(
            css_rgb_channel(tokens[0])?,
            css_rgb_channel(tokens[1])?,
            css_rgb_channel(tokens[2])?,
        ));
    }

    if let Some(arguments) =
        css_function_arguments(&value, "hsl").or_else(|| css_function_arguments(&value, "hsla"))
    {
        let tokens = css_color_tokens(arguments);
        if tokens.len() < 3 {
            return None;
        }
        let hue = css_hue(tokens[0])?;
        let saturation = css_percentage(tokens[1])?;
        let lightness = css_percentage(tokens[2])?;
        let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
        let sector = hue / 60.0;
        let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let (red, green, blue) = match sector as u32 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let match_lightness = lightness - chroma / 2.0;
        let channel = |component: f64| ((component + match_lightness) * 255.0).round() as u8;
        return Some(rgb_hex(channel(red), channel(green), channel(blue)));
    }

    None
}

fn css_function_arguments<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value
        .strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

fn css_color_tokens(arguments: &str) -> Vec<&str> {
    arguments
        .split(|character: char| character.is_ascii_whitespace() || matches!(character, ',' | '/'))
        .filter(|token| !token.is_empty())
        .collect()
}

fn css_rgb_channel(value: &str) -> Option<u8> {
    if let Some(percentage) = value.strip_suffix('%') {
        let percentage = percentage.parse::<f64>().ok()?;
        if !percentage.is_finite() {
            return None;
        }
        return Some((percentage.clamp(0.0, 100.0) * 255.0 / 100.0).round() as u8);
    }
    let channel = value.parse::<f64>().ok()?;
    channel
        .is_finite()
        .then(|| channel.clamp(0.0, 255.0).round() as u8)
}

fn css_percentage(value: &str) -> Option<f64> {
    let percentage = value.strip_suffix('%')?.parse::<f64>().ok()?;
    percentage
        .is_finite()
        .then(|| percentage.clamp(0.0, 100.0) / 100.0)
}

fn css_hue(value: &str) -> Option<f64> {
    let (number, multiplier) = if let Some(number) = value.strip_suffix("deg") {
        (number, 1.0)
    } else if let Some(number) = value.strip_suffix("grad") {
        (number, 0.9)
    } else if let Some(number) = value.strip_suffix("turn") {
        (number, 360.0)
    } else if let Some(number) = value.strip_suffix("rad") {
        (number, 180.0 / std::f64::consts::PI)
    } else {
        (value, 1.0)
    };
    let hue = number.parse::<f64>().ok()? * multiplier;
    hue.is_finite().then(|| hue.rem_euclid(360.0))
}

fn rgb_hex(red: u8, green: u8, blue: u8) -> String {
    format!("#{red:02x}{green:02x}{blue:02x}")
}

fn invalid(error: impl std::fmt::Display) -> SaveError {
    SaveError::InvalidRequest(error.to_string())
}

fn normalized_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        get_note, parse_reading, set_favorite, set_rating, set_read, write_reading,
        writer::write_reading_under_lock,
    };
    use std::{fs, sync::mpsc};

    fn library() -> (tempfile::TempDir, LibraryRoot) {
        let dir = tempfile::TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        (dir, library)
    }

    fn full_capture(url: &str) -> SaveInput {
        SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Article,
            lightweight: false,
            url: url.to_string(),
            media_url: None,
            canonical_url: url.to_string(),
            title: "Captured title".to_string(),
            author: Some("Author".to_string()),
            site: Some("example.com".to_string()),
            saved_at: "2026-08-24T12:00:00.000Z".to_string(),
            markdown: "# Captured title\n\nFull body.".to_string(),
            images: vec![],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            excerpt: Some("Full body.".to_string()),
            word_count: Some(2),
            lang: Some("en".to_string()),
        }
    }

    fn link_metadata_capture(url: &str) -> SaveLinkInput {
        let preview_url = "https://assets.example/social.png".to_string();
        SaveLinkInput {
            url: url.to_string(),
            canonical_url: url.to_string(),
            title: "Captured social title".to_string(),
            author: None,
            site: Some("Example".to_string()),
            saved_at: "2026-08-25T12:00:00.000Z".to_string(),
            images: vec![ImageBytes {
                url: preview_url.clone(),
                content_type: "image/png".to_string(),
                bytes: b"preview".to_vec(),
            }],
            preview_url: Some(preview_url),
            favicon_url: None,
            theme_color: Some("#123456".to_string()),
            excerpt: Some("Captured excerpt".to_string()),
            lang: Some("en".to_string()),
        }
    }

    fn source_profile(attachments: Vec<crate::SourceAttachment>) -> crate::SourceProfile {
        crate::SourceProfile {
            version: 1,
            source_type: "social_post".into(),
            provider: "x".into(),
            source_id: "2102505743278829840".into(),
            author_handle: "@benspringwater".into(),
            published_at: Some("2026-09-22T18:42:00.000Z".into()),
            avatar_asset: None,
            attachments,
        }
    }

    #[test]
    fn source_capture_commits_required_staged_assets_before_the_article() {
        let (_dir, library) = library();
        let staging = tempfile::TempDir::new().unwrap();
        let image_bytes = b"social image bytes";
        let video_bytes = b"social video bytes";
        let image_hash = sha256_hex(image_bytes);
        let video_hash = sha256_hex(video_bytes);
        let image_source = staging.path().join("image.tmp");
        let video_source = staging.path().join("video.tmp");
        fs::write(&image_source, image_bytes).unwrap();
        fs::write(&video_source, video_bytes).unwrap();
        let image_asset = format!("assets/{image_hash}.jpg");
        let video_asset = format!("assets/{video_hash}.mp4");
        let input = SourceCaptureInput {
            capture: full_capture("https://x.com/benspringwater/status/2102505743278829840"),
            source_profile: source_profile(vec![
                crate::SourceAttachment {
                    kind: "image".into(),
                    asset: image_asset.clone(),
                    poster_asset: None,
                    content_type: Some("image/jpeg".into()),
                    width: Some(1200),
                    height: Some(800),
                    alt: Some("A screenshot".into()),
                },
                crate::SourceAttachment {
                    kind: "video".into(),
                    asset: video_asset.clone(),
                    poster_asset: Some(image_asset.clone()),
                    content_type: Some("video/mp4".into()),
                    width: Some(1920),
                    height: Some(1080),
                    alt: None,
                },
            ]),
            staged_assets: vec![
                StagedSourceAsset::new(
                    image_source.clone(),
                    image_asset.clone(),
                    image_hash,
                    image_bytes.len() as u64,
                )
                .unwrap(),
                StagedSourceAsset::new(
                    video_source.clone(),
                    video_asset.clone(),
                    video_hash,
                    video_bytes.len() as u64,
                )
                .unwrap(),
            ],
            expected_article: ExpectedArticleState::Missing,
        };

        let saved = save_source_capture(&library, input).unwrap();
        let metadata = read_metadata(&library.article_path(&saved.id)).unwrap();

        assert_eq!(saved.disposition, SaveDisposition::Saved);
        assert_eq!(metadata.source_profile.unwrap().provider, "x");
        assert_eq!(
            fs::read(library.reading_dir(&saved.id).join(image_asset)).unwrap(),
            image_bytes
        );
        assert_eq!(
            fs::read(library.reading_dir(&saved.id).join(video_asset)).unwrap(),
            video_bytes
        );
        assert!(!image_source.exists());
        assert!(!video_source.exists());
    }

    #[test]
    fn source_capture_converts_a_full_generic_article_and_preserves_user_state() {
        let (_dir, library) = library();
        let url = "https://x.com/example/status/2102505743278829840";
        let mut generic = full_capture(url);
        generic.title = "Generic browser capture".into();
        generic.markdown = "Generic body".into();
        let existing = save_capture(&library, generic).unwrap();
        let mut existing_reading =
            parse_reading(&fs::read_to_string(library.article_path(&existing.id)).unwrap())
                .unwrap();
        existing_reading.metadata.favorite = true;
        let saved_at = existing_reading.metadata.saved_at.clone();
        write_reading(&library, existing_reading.metadata, existing_reading.body).unwrap();
        let expected_article = ExpectedArticleState::Sha256(sha256_hex(
            &fs::read(library.article_path(&existing.id)).unwrap(),
        ));

        let mut social = full_capture(url);
        social.title = "Post by Example".into();
        social.markdown = "Complete post text".into();
        let outcome = save_source_capture(
            &library,
            SourceCaptureInput {
                capture: social,
                source_profile: source_profile(vec![]),
                staged_assets: vec![],
                expected_article,
            },
        )
        .unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&outcome.id)).unwrap()).unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Upgraded);
        assert_eq!(outcome.id, existing.id);
        assert_eq!(reading.metadata.title, "Post by Example");
        assert_eq!(reading.metadata.saved_at, saved_at);
        assert!(reading.metadata.favorite);
        assert_eq!(reading.body, "Complete post text\n");
        assert_eq!(reading.metadata.source_profile.unwrap().provider, "x");
    }

    #[test]
    fn source_capture_repairs_a_corrupt_required_asset() {
        let (_dir, library) = library();
        let staging = tempfile::TempDir::new().unwrap();
        let bytes = b"durable social media";
        let hash = sha256_hex(bytes);
        let asset = format!("assets/{hash}.jpg");
        let first_source = staging.path().join("first.tmp");
        fs::write(&first_source, bytes).unwrap();
        let url = "https://x.com/example/status/2102505743278829840";
        let attachment = crate::SourceAttachment {
            kind: "image".into(),
            asset: asset.clone(),
            poster_asset: None,
            content_type: Some("image/jpeg".into()),
            width: Some(1200),
            height: Some(800),
            alt: None,
        };

        let first = save_source_capture(
            &library,
            SourceCaptureInput {
                capture: full_capture(url),
                source_profile: source_profile(vec![attachment.clone()]),
                staged_assets: vec![StagedSourceAsset::new(
                    first_source,
                    asset.clone(),
                    hash.clone(),
                    bytes.len() as u64,
                )
                .unwrap()],
                expected_article: ExpectedArticleState::Missing,
            },
        )
        .unwrap();
        let final_asset = library.reading_dir(&first.id).join(&asset);
        fs::write(&final_asset, b"corrupt").unwrap();

        let repair_source = staging.path().join("repair.tmp");
        fs::write(&repair_source, bytes).unwrap();
        let expected_article = ExpectedArticleState::Sha256(sha256_hex(
            &fs::read(library.article_path(&first.id)).unwrap(),
        ));
        let repaired = save_source_capture(
            &library,
            SourceCaptureInput {
                capture: full_capture(url),
                source_profile: source_profile(vec![attachment]),
                staged_assets: vec![StagedSourceAsset::new(
                    repair_source,
                    asset,
                    hash,
                    bytes.len() as u64,
                )
                .unwrap()],
                expected_article,
            },
        )
        .unwrap();

        assert_eq!(repaired.disposition, SaveDisposition::Upgraded);
        assert_eq!(fs::read(final_asset).unwrap(), bytes);
    }

    #[test]
    fn source_capture_hash_failure_cleans_staging_without_committing_article() {
        let (_dir, library) = library();
        let staging = tempfile::TempDir::new().unwrap();
        let bytes = b"different bytes than declared";
        let declared_hash = sha256_hex(b"declared bytes");
        let source = staging.path().join("attachment.tmp");
        fs::write(&source, bytes).unwrap();
        let asset = format!("assets/{declared_hash}.mp4");
        let url = "https://x.com/example/status/99";
        let input = SourceCaptureInput {
            capture: full_capture(url),
            source_profile: source_profile(vec![crate::SourceAttachment {
                kind: "video".into(),
                asset: asset.clone(),
                poster_asset: None,
                content_type: Some("video/mp4".into()),
                width: None,
                height: None,
                alt: None,
            }]),
            staged_assets: vec![StagedSourceAsset::new(
                source.clone(),
                asset,
                declared_hash,
                bytes.len() as u64,
            )
            .unwrap()],
            expected_article: ExpectedArticleState::Missing,
        };

        let result = save_source_capture(&library, input);
        let id = url_id(url).unwrap();

        assert!(matches!(result, Err(SaveError::InvalidRequest(_))));
        assert!(!library.article_path(&id).exists());
        assert!(!source.exists());
        if library.assets_dir(&id).is_dir() {
            assert!(fs::read_dir(library.assets_dir(&id)).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".asset.")
            }));
        }
    }

    #[test]
    fn staged_source_asset_rejects_unsafe_targets_and_cleans_the_adopted_file() {
        let staging = tempfile::TempDir::new().unwrap();
        let source = staging.path().join("attachment.tmp");
        let bytes = b"attachment";
        fs::write(&source, bytes).unwrap();

        let result = StagedSourceAsset::new(
            source.clone(),
            format!("assets/../{}.jpg", sha256_hex(bytes)),
            sha256_hex(bytes),
            bytes.len() as u64,
        );

        assert!(matches!(result, Err(SaveError::InvalidRequest(_))));
        assert!(!source.exists());
    }

    #[test]
    fn source_capture_rejects_zero_attachment_dimensions() {
        let (_dir, library) = library();
        let staging = tempfile::TempDir::new().unwrap();
        let bytes = b"image bytes";
        let hash = sha256_hex(bytes);
        let asset = format!("assets/{hash}.jpg");
        let source = staging.path().join("attachment.tmp");
        fs::write(&source, bytes).unwrap();

        let result = save_source_capture(
            &library,
            SourceCaptureInput {
                capture: full_capture("https://x.com/example/status/42"),
                source_profile: source_profile(vec![crate::SourceAttachment {
                    kind: "image".into(),
                    asset: asset.clone(),
                    poster_asset: None,
                    content_type: Some("image/jpeg".into()),
                    width: Some(0),
                    height: Some(100),
                    alt: None,
                }]),
                staged_assets: vec![StagedSourceAsset::new(
                    source.clone(),
                    asset,
                    hash,
                    bytes.len() as u64,
                )
                .unwrap()],
                expected_article: ExpectedArticleState::Missing,
            },
        );

        assert!(matches!(
            result,
            Err(SaveError::InvalidRequest(message)) if message.contains("dimensions")
        ));
        assert!(!source.exists());
    }

    #[test]
    fn full_capture_upgrade_preserves_an_existing_source_profile_when_omitted() {
        let (_dir, library) = library();
        let url = "https://x.com/example/status/42";
        let id = url_id(url).unwrap();
        let mut lightweight = full_capture(url);
        lightweight.lightweight = true;
        lightweight.markdown = "Saved link".into();
        let mut metadata = Metadata {
            format_version: 1,
            id: id.clone(),
            kind: ReadingKind::Article,
            lightweight: true,
            url: url.into(),
            media_url: None,
            preview_asset: None,
            favicon_asset: None,
            theme_color: None,
            canonical_url: url.into(),
            title: "Saved link".into(),
            author: None,
            site: Some("x.com".into()),
            source_profile: Some(source_profile(vec![])),
            saved_at: "2026-09-22T18:42:00.000Z".into(),
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
        };
        write_reading(&library, metadata.clone(), lightweight.markdown).unwrap();

        let upgraded = save_capture(&library, full_capture(url)).unwrap();
        metadata = read_metadata(&library.article_path(&id)).unwrap();

        assert_eq!(upgraded.disposition, SaveDisposition::Upgraded);
        assert_eq!(metadata.source_profile.unwrap().provider, "x");
    }

    #[test]
    fn website_theme_colors_normalize_to_canonical_srgb_hex() {
        assert_eq!(normalize_theme_color(" #AbC ").as_deref(), Some("#aabbcc"));
        assert_eq!(
            normalize_theme_color("#123456cc").as_deref(),
            Some("#123456")
        );
        assert_eq!(
            normalize_theme_color("rgb(100% 0% 50%)").as_deref(),
            Some("#ff0080")
        );
        assert_eq!(
            normalize_theme_color("hsl(210, 50%, 40%)").as_deref(),
            Some("#336699")
        );
        assert_eq!(normalize_theme_color("transparent"), None);
        assert_eq!(normalize_theme_color("var(--brand)"), None);
    }

    fn assert_staging_empty(library: &LibraryRoot) {
        let staging = library.path().join(IMPORT_STAGING_DIRECTORY);
        if staging.is_dir() {
            assert_eq!(
                fs::read_dir(staging).unwrap().count(),
                0,
                "temporary video imports must be cleaned up"
            );
        }
    }

    fn mp4_video_bytes(payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32 + payload.len());
        bytes.extend_from_slice(&24_u32.to_be_bytes());
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"isom");
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(b"isom");
        bytes.extend_from_slice(b"mp42");
        bytes.extend_from_slice(&u32::try_from(8 + payload.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(b"mdat");
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn link_uses_article_identity_and_is_persistently_lightweight() {
        let (_dir, library) = library();
        let url = "https://www.example.com/post/?utm_source=paste";

        let result = import_link(&library, url).unwrap();
        let reading =
            parse_reading(&std::fs::read_to_string(library.article_path(&result.id)).unwrap())
                .unwrap();

        assert_eq!(result.disposition, SaveDisposition::Saved);
        assert_eq!(result.id, url_id(url).unwrap());
        assert!(reading.metadata.lightweight);
        assert_eq!(reading.metadata.url, "https://example.com/post");
    }

    #[test]
    fn import_options_preserve_title_saved_at_tags_favorite_and_note() {
        let (_dir, library) = library();
        let saved_at = "2020-01-02T03:04:05.000Z";
        let note = "## Why I saved this\n\nA durable personal note.";

        let result = import_text_with_options(
            &library,
            "A selected passage.",
            ImportOptions {
                title: Some("  Original title  ".to_string()),
                saved_at: Some(saved_at.to_string()),
                state: ImportedReadingState {
                    favorite: true,
                    tags: vec!["  inspiration  ".to_string(), "local-first".to_string()],
                    note_markdown: Some(note.to_string()),
                },
            },
        )
        .unwrap();
        let reading = read_metadata(&library.article_path(&result.id)).unwrap();

        assert_eq!(result.disposition, SaveDisposition::Saved);
        assert_eq!(reading.title, "Original title");
        assert_eq!(reading.saved_at, saved_at);
        assert!(reading.favorite);
        assert_eq!(reading.tags, vec!["inspiration", "local-first"]);
        assert_eq!(
            get_note(&library, &result.id).unwrap().as_deref(),
            Some(note)
        );
    }

    #[test]
    fn duplicate_import_does_not_validate_or_replace_existing_state() {
        let (_dir, library) = library();
        let first = import_text_with_options(
            &library,
            "The same quote.",
            ImportOptions {
                title: Some("First title".to_string()),
                saved_at: Some("2020-01-02T03:04:05.000Z".to_string()),
                state: ImportedReadingState {
                    favorite: true,
                    tags: vec!["kept".to_string()],
                    note_markdown: Some("kept note".to_string()),
                },
            },
        )
        .unwrap();

        let duplicate = import_text_with_options(
            &library,
            "The  same quote.",
            ImportOptions {
                title: Some("Replacement title".to_string()),
                saved_at: Some("2030-01-02T03:04:05.000Z".to_string()),
                state: ImportedReadingState {
                    favorite: false,
                    // Deliberately invalid: duplicate detection must return
                    // before imported state is validated or written.
                    tags: vec!["Must Not Replace".to_string()],
                    note_markdown: Some("replacement note".to_string()),
                },
            },
        )
        .unwrap();
        let reading = read_metadata(&library.article_path(&first.id)).unwrap();

        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(reading.title, "First title");
        assert_eq!(reading.saved_at, "2020-01-02T03:04:05.000Z");
        assert!(reading.favorite);
        assert_eq!(reading.tags, vec!["kept"]);
        assert_eq!(
            get_note(&library, &first.id).unwrap().as_deref(),
            Some("kept note")
        );
    }

    #[test]
    fn invalid_imported_tags_fail_before_creating_a_reading() {
        let (_dir, library) = library();
        let input = full_capture("https://example.com/invalid-imported-tag");
        let id = url_id(&input.url).unwrap();

        let result = import_reading(
            &library,
            input,
            ImportedReadingState {
                tags: vec!["Not Valid".to_string()],
                ..ImportedReadingState::default()
            },
        );

        assert!(matches!(result, Err(SaveError::InvalidRequest(_))));
        assert!(!library.article_path(&id).exists());
        assert!(!library.reading_dir(&id).exists());
    }

    #[test]
    fn import_reading_falls_back_from_a_blank_saved_at() {
        let (_dir, library) = library();
        let mut input = full_capture("https://example.com/missing-import-date");
        input.saved_at = " \n\t ".to_string();

        let result = import_reading(&library, input, ImportedReadingState::default()).unwrap();
        let reading = read_metadata(&library.article_path(&result.id)).unwrap();

        assert!(!reading.saved_at.trim().is_empty());
        assert_ne!(reading.saved_at, " \n\t ");
    }

    #[test]
    fn import_without_a_note_preserves_a_preexisting_precommit_note() {
        let (_dir, library) = library();
        let input = full_capture("https://example.com/retried-import");
        let id = url_id(&input.url).unwrap();
        fs::create_dir_all(library.assets_dir(&id)).unwrap();
        let preexisting = "note supplied by external sync";
        fs::write(library.note_path(&id), preexisting).unwrap();

        let result = import_reading(&library, input, ImportedReadingState::default()).unwrap();

        assert_eq!(result.disposition, SaveDisposition::Saved);
        assert_eq!(
            get_note(&library, &id).unwrap().as_deref(),
            Some(preexisting)
        );
    }

    #[test]
    fn full_capture_upgrades_link_and_preserves_user_state() {
        let (_dir, library) = library();
        let url = "https://example.com/post";
        let imported = import_link(&library, url).unwrap();
        let original = read_metadata(&library.article_path(&imported.id)).unwrap();

        let index_dir = tempfile::TempDir::new().unwrap();
        let conn = crate::open_index(&index_dir.path().join("index.db")).unwrap();
        crate::rebuild(&conn, &library).unwrap();
        set_read(&library, &conn, &imported.id, true).unwrap();
        set_rating(&library, &conn, &imported.id, 4).unwrap();
        crate::add_tag(&library, &conn, &imported.id, "later").unwrap();

        let result = save_capture(&library, full_capture(url)).unwrap();
        let reading =
            parse_reading(&std::fs::read_to_string(library.article_path(&result.id)).unwrap())
                .unwrap();

        assert_eq!(result.disposition, SaveDisposition::Upgraded);
        assert!(!reading.metadata.lightweight);
        assert_eq!(reading.metadata.saved_at, original.saved_at);
        assert!(reading.metadata.read_at.is_some());
        assert_eq!(reading.metadata.rating, 4);
        assert_eq!(reading.metadata.tags, vec!["later"]);
        assert_eq!(reading.metadata.title, "Captured title");
        assert_eq!(reading.body, "# Captured title\n\nFull body.\n");
    }

    #[test]
    fn metadata_enrichment_preserves_lightweight_body_and_user_state() {
        let (_dir, library) = library();
        let url = "https://example.com/enrich-me";
        let saved_at = "2020-01-02T03:04:05.000Z";
        let imported = import_link_with_options(
            &library,
            url,
            ImportOptions {
                title: Some("Imported title".to_string()),
                saved_at: Some(saved_at.to_string()),
                state: ImportedReadingState {
                    favorite: true,
                    tags: vec!["original".to_string()],
                    note_markdown: Some("original note".to_string()),
                },
            },
        )
        .unwrap();
        let mut existing =
            parse_reading(&fs::read_to_string(library.article_path(&imported.id)).unwrap())
                .unwrap();
        existing.metadata.author = Some("Existing author".to_string());
        existing.metadata.site = Some("Existing site".to_string());
        existing.metadata.excerpt = Some("Existing excerpt".to_string());
        existing.metadata.word_count = Some(42);
        existing.metadata.lang = Some("ga".to_string());
        let custom_body = "A locally edited lightweight body.\n";
        crate::write_reading(&library, existing.metadata, custom_body.to_string()).unwrap();

        let preview_url = "https://assets.example/social.png".to_string();
        let favicon_url = "https://assets.example/favicon.ico".to_string();
        let outcome = import_link_capture(
            &library,
            SaveLinkInput {
                url: url.to_string(),
                canonical_url: url.to_string(),
                title: "Captured social title".to_string(),
                author: None,
                site: None,
                saved_at: "2026-08-25T12:00:00.000Z".to_string(),
                images: vec![
                    ImageBytes {
                        url: preview_url.clone(),
                        content_type: "image/png".to_string(),
                        bytes: b"preview".to_vec(),
                    },
                    ImageBytes {
                        url: favicon_url.clone(),
                        content_type: "image/x-icon".to_string(),
                        bytes: b"favicon".to_vec(),
                    },
                ],
                preview_url: Some(preview_url),
                favicon_url: Some(favicon_url),
                theme_color: Some("#123456".to_string()),
                excerpt: None,
                lang: None,
            },
            ImportedReadingState::default(),
        )
        .unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&imported.id)).unwrap())
                .unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Upgraded);
        assert_eq!(reading.body, custom_body);
        assert_eq!(reading.metadata.saved_at, saved_at);
        assert!(reading.metadata.favorite);
        assert_eq!(reading.metadata.tags, vec!["original"]);
        assert_eq!(reading.metadata.title, "Captured social title");
        assert_eq!(reading.metadata.theme_color.as_deref(), Some("#123456"));
        assert_eq!(reading.metadata.author.as_deref(), Some("Existing author"));
        assert_eq!(reading.metadata.site.as_deref(), Some("Existing site"));
        assert_eq!(
            reading.metadata.excerpt.as_deref(),
            Some("Existing excerpt")
        );
        assert_eq!(reading.metadata.word_count, Some(42));
        assert_eq!(reading.metadata.lang.as_deref(), Some("ga"));
        assert!(reading.metadata.preview_asset.is_some());
        assert!(reading.metadata.favicon_asset.is_some());
        assert_eq!(
            get_note(&library, &imported.id).unwrap().as_deref(),
            Some("original note")
        );
    }

    #[test]
    fn conditional_metadata_enrichment_requires_the_exact_article_snapshot() {
        let (_dir, library) = library();
        let url = "https://example.com/conditional-enrichment";
        let imported = import_link(&library, url).unwrap();
        let article_path = library.article_path(&imported.id);
        let planned_hash = sha256_hex(&fs::read(&article_path).unwrap());

        // This edit deliberately leaves the reading lightweight and without
        // enrichment fields. A structural guard would miss it; the complete
        // article snapshot must still prevent overwriting the local change.
        let mut changed = parse_reading(&fs::read_to_string(&article_path).unwrap()).unwrap();
        changed.metadata.title = "Locally changed title".to_string();
        crate::write_reading(
            &library,
            changed.metadata,
            "Locally changed lightweight body.".to_string(),
        )
        .unwrap();

        let stale = import_link_capture_if_unchanged(
            &library,
            link_metadata_capture(url),
            ImportedReadingState::default(),
            &planned_hash,
        )
        .unwrap();
        let preserved = parse_reading(&fs::read_to_string(&article_path).unwrap()).unwrap();
        assert_eq!(stale.disposition, SaveDisposition::Duplicate);
        assert_eq!(preserved.metadata.title, "Locally changed title");
        assert_eq!(preserved.body, "Locally changed lightweight body.\n");
        assert!(preserved.metadata.preview_asset.is_none());
        assert!(preserved.metadata.theme_color.is_none());
        assert_eq!(
            fs::read_dir(library.assets_dir(&imported.id))
                .unwrap()
                .count(),
            0
        );

        let current_hash = sha256_hex(&fs::read(&article_path).unwrap());
        let applied = import_link_capture_if_unchanged(
            &library,
            link_metadata_capture(url),
            ImportedReadingState::default(),
            &current_hash,
        )
        .unwrap();
        assert_eq!(applied.disposition, SaveDisposition::Upgraded);
        let enriched = read_metadata(&article_path).unwrap();
        assert!(enriched.preview_asset.is_some());
        assert_eq!(enriched.theme_color.as_deref(), Some("#123456"));
    }

    #[test]
    fn imported_state_cannot_replace_state_or_note_during_upgrade() {
        let (_dir, library) = library();
        let url = "https://example.com/imported-upgrade";
        let original_saved_at = "2020-01-02T03:04:05.000Z";
        let imported = import_link_with_options(
            &library,
            url,
            ImportOptions {
                title: Some("Original lightweight title".to_string()),
                saved_at: Some(original_saved_at.to_string()),
                state: ImportedReadingState {
                    favorite: true,
                    tags: vec!["original".to_string()],
                    note_markdown: Some("original note".to_string()),
                },
            },
        )
        .unwrap();

        let result = import_reading(
            &library,
            full_capture(url),
            ImportedReadingState {
                favorite: false,
                // Invalid state is ignored entirely on an upgrade.
                tags: vec!["Must Not Replace".to_string()],
                note_markdown: Some("replacement note".to_string()),
            },
        )
        .unwrap();
        let reading = read_metadata(&library.article_path(&result.id)).unwrap();

        assert_eq!(result.disposition, SaveDisposition::Upgraded);
        assert_eq!(result.id, imported.id);
        assert_eq!(reading.saved_at, original_saved_at);
        assert!(reading.favorite);
        assert_eq!(reading.tags, vec!["original"]);
        assert_eq!(reading.title, "Captured title");
        assert_eq!(
            get_note(&library, &result.id).unwrap().as_deref(),
            Some("original note")
        );
    }

    #[test]
    fn normal_article_duplicate_stays_structured() {
        let (_dir, library) = library();
        let url = "https://example.com/post";
        save_capture(&library, full_capture(url)).unwrap();

        let duplicate = save_capture(&library, full_capture(url)).unwrap();
        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(duplicate.id, url_id(url).unwrap());
    }

    #[test]
    fn text_import_has_content_identity_and_synthetic_origin() {
        let (_dir, library) = library();
        let first = import_text(&library, " A selected\npassage. ", Some("Notes.txt")).unwrap();
        let duplicate = import_text(&library, "A  selected passage.", None).unwrap();
        let reading = read_metadata(&library.article_path(&first.id)).unwrap();

        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(first.id, duplicate.id);
        assert!(reading.url.starts_with("cuttings://local/quote/"));
        assert_eq!(reading.kind, ReadingKind::Quote);
        assert_eq!(reading.title, "Notes.txt");
    }

    #[test]
    fn image_import_writes_bytes_and_deduplicates_by_content() {
        let (_dir, library) = library();
        let bytes = b"normalized image bytes".to_vec();
        let first = import_image(&library, bytes.clone(), "image/png", "Pasted image").unwrap();
        let duplicate = import_image(&library, bytes.clone(), "image/png", "Other title").unwrap();
        let reading = read_metadata(&library.article_path(&first.id)).unwrap();

        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(first.id, duplicate.id);
        assert!(reading.url.starts_with("cuttings://local/image/"));
        assert_eq!(reading.kind, ReadingKind::Image);
        let asset = reading.preview_asset.unwrap();
        assert_eq!(
            std::fs::read(library.reading_dir(&first.id).join(asset)).unwrap(),
            bytes
        );
    }

    #[test]
    fn imported_image_asset_failure_does_not_commit_the_reading() {
        let (_dir, library) = library();
        let bytes = b"required image bytes".to_vec();
        let content_hash = sha256_hex(&bytes);
        let origin = format!("cuttings://local/image/{content_hash}");
        let id = media_id(ReadingKind::Image, &origin, &origin).unwrap();
        let blocked_asset = library.assets_dir(&id).join(format!("{content_hash}.png"));
        fs::create_dir_all(&blocked_asset).unwrap();

        let result = import_image(&library, bytes, "image/png", "Required image");

        assert!(matches!(result, Err(SaveError::Storage(_))));
        assert!(!library.article_path(&id).exists());
        assert!(
            blocked_asset.is_dir(),
            "the existing path must be untouched"
        );
        assert!(fs::read_dir(library.assets_dir(&id)).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".asset.")
        }));
    }

    #[test]
    fn browser_capture_keeps_best_effort_image_handling() {
        let (_dir, library) = library();
        let url = "https://example.com/browser-image-failure";
        let image_url = "https://example.com/image.png";
        let bytes = b"browser image bytes".to_vec();
        let content_hash = sha256_hex(&bytes);
        let id = url_id(url).unwrap();
        let blocked_asset = library.assets_dir(&id).join(format!("{content_hash}.png"));
        fs::create_dir_all(&blocked_asset).unwrap();
        let mut input = full_capture(url);
        input.markdown = format!("![Browser image]({image_url})");
        input.images = vec![ImageBytes {
            url: image_url.to_string(),
            content_type: "image/png".to_string(),
            bytes,
        }];

        let result = save_capture(&library, input).unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&id)).unwrap()).unwrap();

        assert_eq!(result.disposition, SaveDisposition::Saved);
        assert!(reading.body.contains(image_url));
        assert_eq!(reading.metadata.preview_asset, None);
        assert!(blocked_asset.is_dir());
    }

    #[test]
    fn browser_capture_stores_social_preview_and_favicon_outside_markdown() {
        let (_dir, library) = library();
        let url = "https://example.com/meta-assets";
        let preview_url = "https://cdn.example.com/social.png";
        let favicon_url = "https://example.com/favicon.ico";
        let preview_bytes = b"social image bytes".to_vec();
        let favicon_bytes = b"favicon bytes".to_vec();
        let mut input = full_capture(url);
        input.markdown =
            format!("[Social source]({preview_url})\n\n![Remote variant]({preview_url}?width=640)");
        let original_markdown = input.markdown.clone();
        input.images = vec![
            ImageBytes {
                url: preview_url.to_string(),
                content_type: "image/png".to_string(),
                bytes: preview_bytes.clone(),
            },
            ImageBytes {
                url: favicon_url.to_string(),
                content_type: "image/x-icon".to_string(),
                bytes: favicon_bytes.clone(),
            },
        ];
        input.preview_url = Some(preview_url.to_string());
        input.favicon_url = Some(favicon_url.to_string());

        let saved = save_capture(&library, input).unwrap();
        let article = fs::read_to_string(library.article_path(&saved.id)).unwrap();
        let reading = parse_reading(&article).unwrap();

        assert_eq!(reading.body.trim_end(), original_markdown);
        let preview_asset = reading.metadata.preview_asset.unwrap();
        let favicon_asset = reading.metadata.favicon_asset.unwrap();
        assert_eq!(
            fs::read(library.reading_dir(&saved.id).join(preview_asset)).unwrap(),
            preview_bytes
        );
        assert_eq!(
            fs::read(library.reading_dir(&saved.id).join(favicon_asset)).unwrap(),
            favicon_bytes
        );
    }

    #[test]
    fn origin_image_import_preserves_origin_and_uses_a_local_asset_identity() {
        let (_dir, library) = library();
        let bytes = b"origin-aware image bytes".to_vec();
        let content_hash = sha256_hex(&bytes);
        let relative_asset = format!("assets/{content_hash}.png");
        let media_reference = format!("cuttings-asset:{relative_asset}");

        let imported = import_image_from_origin_with_options(
            &library,
            bytes.clone(),
            "image/png",
            "https://www.example.com/gallery/photo.png?utm_source=export",
            ImportOptions {
                title: Some("Exported image".to_string()),
                ..ImportOptions::default()
            },
        )
        .unwrap();
        let article = fs::read_to_string(library.article_path(&imported.id)).unwrap();
        let reading = parse_reading(&article).unwrap();

        assert_eq!(imported.disposition, SaveDisposition::Saved);
        assert_eq!(
            imported.id,
            media_id(
                ReadingKind::Image,
                "https://example.com/gallery/photo.png",
                &media_reference
            )
            .unwrap()
        );
        assert_eq!(
            reading.metadata.url,
            "https://example.com/gallery/photo.png"
        );
        assert_eq!(reading.metadata.canonical_url, reading.metadata.url);
        assert_eq!(reading.metadata.site.as_deref(), Some("example.com"));
        assert_eq!(
            reading.metadata.media_url.as_deref(),
            Some(media_reference.as_str())
        );
        assert_eq!(
            reading.metadata.preview_asset.as_deref(),
            Some(relative_asset.as_str())
        );
        assert_eq!(
            reading.body,
            format!("![Imported image]({relative_asset})\n")
        );
        assert_eq!(
            fs::read(library.reading_dir(&imported.id).join(&relative_asset)).unwrap(),
            bytes
        );
    }

    #[test]
    fn origin_image_identity_combines_normalized_origin_and_bytes() {
        let (_dir, library) = library();
        let bytes = b"shared image bytes".to_vec();

        let first = import_image_from_origin_with_options(
            &library,
            bytes.clone(),
            "image/jpeg",
            "https://www.example.com/board?utm_source=one",
            ImportOptions::default(),
        )
        .unwrap();
        let duplicate = import_image_from_origin_with_options(
            &library,
            bytes.clone(),
            "image/jpeg",
            "https://example.com/board",
            ImportOptions::default(),
        )
        .unwrap();
        let other_origin = import_image_from_origin_with_options(
            &library,
            bytes,
            "image/jpeg",
            "https://elsewhere.example/board",
            ImportOptions::default(),
        )
        .unwrap();

        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(duplicate.id, first.id);
        assert_eq!(other_origin.disposition, SaveDisposition::Saved);
        assert_ne!(other_origin.id, first.id);
    }

    #[test]
    fn origin_image_does_not_borrow_the_page_extension() {
        let (_dir, library) = library();
        let imported = import_image_from_origin_with_options(
            &library,
            b"unknown image representation".to_vec(),
            "application/octet-stream",
            "https://example.com/gallery/page.html",
            ImportOptions::default(),
        )
        .unwrap();
        let metadata = read_metadata(&library.article_path(&imported.id)).unwrap();

        assert!(metadata.media_url.unwrap().ends_with(".bin"));
        assert!(metadata.preview_asset.unwrap().ends_with(".bin"));
    }

    #[test]
    fn video_import_streams_to_a_content_addressed_local_asset() {
        let (_dir, library) = library();
        let source_dir = tempfile::TempDir::new().unwrap();
        let source = source_dir.path().join("private-original-name.mp4");
        let mut bytes = vec![0x5a; VIDEO_COPY_BUFFER_SIZE * 2 + 137];
        bytes[0..8].copy_from_slice(b"ftypmp42");
        fs::write(&source, &bytes).unwrap();

        let imported = import_video_file(
            &library,
            &source,
            " Video/MP4; codecs=avc1 ",
            " Local clip ",
        )
        .unwrap();
        let article = fs::read_to_string(library.article_path(&imported.id)).unwrap();
        let reading = parse_reading(&article).unwrap();
        let content_hash = sha256_hex(&bytes);
        let relative_asset = format!("assets/{content_hash}.mp4");

        assert_eq!(imported.disposition, SaveDisposition::Saved);
        assert_eq!(imported.id, local_video_id(&content_hash));
        assert_eq!(reading.metadata.kind, ReadingKind::Video);
        assert_eq!(
            reading.metadata.url,
            format!("cuttings://local/video/{content_hash}")
        );
        assert_eq!(reading.metadata.canonical_url, reading.metadata.url);
        assert_eq!(reading.metadata.author, None);
        assert_eq!(reading.metadata.site, None);
        assert_eq!(reading.metadata.lang, None);
        assert_eq!(reading.metadata.excerpt, None);
        assert_eq!(reading.metadata.word_count, None);
        assert_eq!(reading.metadata.title, "Local clip");
        assert_eq!(
            reading.metadata.media_url.as_deref(),
            Some(format!("cuttings-asset:{relative_asset}").as_str())
        );
        assert_eq!(reading.metadata.preview_asset, None);
        assert_eq!(reading.body, format!("[Play video]({relative_asset})\n"));
        assert_eq!(
            fs::read(library.reading_dir(&imported.id).join(&relative_asset)).unwrap(),
            bytes
        );
        assert!(
            !article.contains(source.to_string_lossy().as_ref()),
            "the caller's source path must never be persisted"
        );
        assert_staging_empty(&library);
    }

    #[test]
    fn browser_video_import_streams_multiple_chunks_to_the_exact_local_asset() {
        let (_dir, library) = library();
        let expected = mp4_video_bytes(b"first browser video chunk and the second chunk");
        let (first, second) = expected.split_at(11);

        let mut import = begin_browser_video_import(
            &library,
            BrowserVideoImportInput {
                content_type: "video/mp4".to_string(),
                expected_bytes: Some(expected.len() as u64),
                origin_url: "https://www.example.com/clips/one?utm_source=feed".to_string(),
                canonical_url: "https://example.com/canonical-clip#player".to_string(),
                title: "Browser clip".to_string(),
                author: Some("Ed Example".to_string()),
                site: Some("Example Studio".to_string()),
                theme_color: Some("#123456".to_string()),
                lang: Some("en-IE".to_string()),
                excerpt: Some("A locally captured browser clip.".to_string()),
                word_count: Some(5),
                saved_at: "2026-08-25T12:00:00.000Z".to_string(),
            },
        )
        .unwrap();
        import.append(first).unwrap();
        import.append(second).unwrap();
        let imported = import.finish().unwrap();

        let reading = read_metadata(&library.article_path(&imported.id)).unwrap();
        assert_eq!(reading.canonical_url, "https://example.com/canonical-clip");
        assert_eq!(reading.author.as_deref(), Some("Ed Example"));
        assert_eq!(reading.site.as_deref(), Some("Example Studio"));
        assert_eq!(reading.theme_color.as_deref(), Some("#123456"));
        assert_eq!(reading.lang.as_deref(), Some("en-IE"));
        assert_eq!(
            reading.excerpt.as_deref(),
            Some("A locally captured browser clip.")
        );
        assert_eq!(reading.word_count, Some(5));
        let media_reference = reading.media_url.unwrap();
        let relative_asset = media_reference.strip_prefix(LOCAL_ASSET_SCHEME).unwrap();
        assert_eq!(
            fs::read(library.reading_dir(&imported.id).join(relative_asset)).unwrap(),
            expected
        );
        assert_staging_empty(&library);
    }

    #[test]
    fn browser_video_import_deduplicates_by_content_within_its_origin() {
        let (_dir, library) = library();
        let bytes = mp4_video_bytes(b"same browser bytes");
        let saved_at = "2026-08-25T12:00:00.000Z";

        let mut first = begin_browser_video_import(
            &library,
            BrowserVideoImportInput {
                content_type: "video/mp4".to_string(),
                expected_bytes: None,
                origin_url: "https://www.example.com/clips/one?utm_source=feed".to_string(),
                canonical_url: "javascript:alert('not canonical')".to_string(),
                title: "First title".to_string(),
                author: None,
                site: None,
                theme_color: None,
                lang: None,
                excerpt: None,
                word_count: None,
                saved_at: saved_at.to_string(),
            },
        )
        .unwrap();
        first.append(&bytes).unwrap();
        let first = first.finish().unwrap();

        let original = read_metadata(&library.article_path(&first.id)).unwrap();
        let original_reference = original.media_url.as_deref().unwrap();
        let original_relative = original_reference.strip_prefix(LOCAL_ASSET_SCHEME).unwrap();
        let original_asset = library.reading_dir(&first.id).join(original_relative);
        fs::remove_file(&original_asset).unwrap();

        let mut duplicate = begin_browser_video_import(
            &library,
            BrowserVideoImportInput {
                content_type: "video/quicktime".to_string(),
                expected_bytes: Some(bytes.len() as u64),
                origin_url: "https://example.com/clips/one".to_string(),
                canonical_url: "https://example.com/clips/one".to_string(),
                title: "Duplicate title".to_string(),
                author: None,
                site: None,
                theme_color: None,
                lang: None,
                excerpt: None,
                word_count: None,
                saved_at: "2027-01-01T00:00:00.000Z".to_string(),
            },
        )
        .unwrap();
        duplicate.append(&bytes).unwrap();
        let duplicate = duplicate.finish().unwrap();

        let mut other_origin = begin_browser_video_import(
            &library,
            BrowserVideoImportInput {
                content_type: "video/mp4".to_string(),
                expected_bytes: Some(bytes.len() as u64),
                origin_url: "https://elsewhere.example/clips/one".to_string(),
                canonical_url: "https://elsewhere.example/clips/one".to_string(),
                title: "Elsewhere".to_string(),
                author: None,
                site: None,
                theme_color: None,
                lang: None,
                excerpt: None,
                word_count: None,
                saved_at: saved_at.to_string(),
            },
        )
        .unwrap();
        other_origin.append(&bytes).unwrap();
        let other_origin = other_origin.finish().unwrap();

        let reading = read_metadata(&library.article_path(&first.id)).unwrap();
        assert_eq!(first.disposition, SaveDisposition::Saved);
        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(duplicate.id, first.id);
        assert_ne!(other_origin.id, first.id);
        assert_eq!(reading.url, "https://example.com/clips/one");
        assert_eq!(reading.canonical_url, reading.url);
        assert_eq!(reading.site.as_deref(), Some("example.com"));
        assert_eq!(reading.title, "First title");
        assert_eq!(reading.saved_at, saved_at);
        assert_eq!(reading.media_url.as_deref(), Some(original_reference));
        assert_eq!(fs::read(&original_asset).unwrap(), bytes);
        assert_eq!(
            fs::read_dir(library.assets_dir(&first.id)).unwrap().count(),
            1
        );
        assert_staging_empty(&library);
    }

    #[test]
    fn browser_video_import_rejects_non_video_bytes_with_a_video_content_type() {
        let (_dir, library) = library();
        let bytes = b"not a video";
        let mut import = begin_browser_video_import(
            &library,
            BrowserVideoImportInput {
                content_type: "video/mp4".to_string(),
                expected_bytes: Some(bytes.len() as u64),
                origin_url: "https://example.com/not-a-video".to_string(),
                canonical_url: "https://example.com/not-a-video".to_string(),
                title: "Not a video".to_string(),
                author: None,
                site: None,
                theme_color: None,
                lang: None,
                excerpt: None,
                word_count: None,
                saved_at: "2026-08-25T12:00:00.000Z".to_string(),
            },
        )
        .unwrap();
        import.append(bytes).unwrap();

        assert!(matches!(
            import.finish(),
            Err(SaveError::InvalidRequest(message))
                if message.contains("does not contain a valid video")
        ));
        assert_staging_empty(&library);
    }

    #[test]
    fn browser_video_import_rejects_empty_mismatched_and_oversized_streams() {
        let (_dir, library) = library();
        let input = |expected_bytes| BrowserVideoImportInput {
            content_type: "video/mp4".to_string(),
            expected_bytes,
            origin_url: "https://example.com/clips/failing".to_string(),
            canonical_url: "https://example.com/clips/failing".to_string(),
            title: "Failing clip".to_string(),
            author: None,
            site: None,
            theme_color: None,
            lang: None,
            excerpt: None,
            word_count: None,
            saved_at: "2026-08-25T12:00:00.000Z".to_string(),
        };

        let empty = begin_browser_video_import(&library, input(None)).unwrap();
        assert!(matches!(empty.finish(), Err(SaveError::InvalidRequest(_))));
        assert_staging_empty(&library);

        let mut mismatched = begin_browser_video_import(&library, input(Some(8))).unwrap();
        mismatched.append(b"short").unwrap();
        assert!(matches!(
            mismatched.finish(),
            Err(SaveError::InvalidRequest(_))
        ));
        assert_staging_empty(&library);

        let mut exceeded = begin_browser_video_import(&library, input(Some(4))).unwrap();
        assert!(matches!(
            exceeded.append(b"12345"),
            Err(SaveError::InvalidRequest(_))
        ));
        drop(exceeded);
        assert_staging_empty(&library);

        assert!(matches!(
            begin_browser_video_import(&library, input(Some(MAX_BROWSER_VIDEO_BYTES + 1))),
            Err(SaveError::InvalidRequest(_))
        ));
        assert_staging_empty(&library);
    }

    #[test]
    fn origin_video_import_preserves_origin_asset_and_scoped_identity() {
        let (_dir, library) = library();
        let source_dir = tempfile::TempDir::new().unwrap();
        let source = source_dir.path().join("private-export-name.mov");
        let bytes = b"origin-aware local video bytes";
        fs::write(&source, bytes).unwrap();
        let content_hash = sha256_hex(bytes);
        let relative_asset = format!("assets/{content_hash}.mov");
        let media_reference = format!("cuttings-asset:{relative_asset}");

        let imported = import_video_file_from_origin_with_options(
            &library,
            &source,
            "video/quicktime",
            "https://www.example.com/clips/one?utm_source=export",
            ImportOptions {
                title: Some("Exported video".to_string()),
                ..ImportOptions::default()
            },
        )
        .unwrap();
        let article = fs::read_to_string(library.article_path(&imported.id)).unwrap();
        let reading = parse_reading(&article).unwrap();

        assert_eq!(imported.disposition, SaveDisposition::Saved);
        assert_eq!(
            imported.id,
            media_id(
                ReadingKind::Video,
                "https://example.com/clips/one",
                &media_reference
            )
            .unwrap()
        );
        assert_eq!(reading.metadata.url, "https://example.com/clips/one");
        assert_eq!(reading.metadata.canonical_url, reading.metadata.url);
        assert_eq!(reading.metadata.author, None);
        assert_eq!(reading.metadata.site.as_deref(), Some("example.com"));
        assert_eq!(reading.metadata.lang, None);
        assert_eq!(reading.metadata.excerpt, None);
        assert_eq!(reading.metadata.word_count, None);
        assert_eq!(
            reading.metadata.media_url.as_deref(),
            Some(media_reference.as_str())
        );
        assert_eq!(reading.body, format!("[Play video]({relative_asset})\n"));
        assert_eq!(
            fs::read(library.reading_dir(&imported.id).join(&relative_asset)).unwrap(),
            bytes
        );
        assert!(!article.contains(source.to_string_lossy().as_ref()));

        let duplicate = import_video_file_from_origin_with_options(
            &library,
            &source,
            "video/mp4",
            "https://example.com/clips/one",
            ImportOptions::default(),
        )
        .unwrap();
        let other_origin = import_video_file_from_origin_with_options(
            &library,
            &source,
            "video/quicktime",
            "https://elsewhere.example/clips/one",
            ImportOptions::default(),
        )
        .unwrap();

        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(duplicate.id, imported.id);
        assert_eq!(other_origin.disposition, SaveDisposition::Saved);
        assert_ne!(other_origin.id, imported.id);
        assert_staging_empty(&library);
    }

    #[test]
    fn video_import_deduplicates_identical_bytes_across_names_and_types() {
        let (_dir, library) = library();
        let source_dir = tempfile::TempDir::new().unwrap();
        let first_path = source_dir.path().join("first.mp4");
        let second_path = source_dir.path().join("renamed.mov");
        let bytes = b"the exact same local video bytes";
        fs::write(&first_path, bytes).unwrap();
        fs::write(&second_path, bytes).unwrap();

        let first = import_video_file(&library, &first_path, "video/mp4", "First title").unwrap();
        let duplicate =
            import_video_file(&library, &second_path, "video/quicktime", "Different title")
                .unwrap();

        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(duplicate.id, first.id);
        let reading = read_metadata(&library.article_path(&first.id)).unwrap();
        assert_eq!(reading.title, "First title");
        assert!(reading.media_url.unwrap().ends_with(".mp4"));
        assert_eq!(
            fs::read_dir(library.assets_dir(&first.id)).unwrap().count(),
            1
        );
        assert_staging_empty(&library);
    }

    #[test]
    fn imported_video_remains_after_the_source_is_deleted() {
        let (_dir, library) = library();
        let source_dir = tempfile::TempDir::new().unwrap();
        let source = source_dir.path().join("delete-me.webm");
        let bytes = b"portable copied video";
        fs::write(&source, bytes).unwrap();

        let imported = import_video_file(&library, &source, "video/webm", "Portable clip").unwrap();
        fs::remove_file(&source).unwrap();

        let metadata = read_metadata(&library.article_path(&imported.id)).unwrap();
        let reference = metadata.media_url.unwrap();
        let relative_asset = reference.strip_prefix(LOCAL_ASSET_SCHEME).unwrap();
        assert_eq!(
            fs::read(library.reading_dir(&imported.id).join(relative_asset)).unwrap(),
            bytes
        );
    }

    #[test]
    fn video_import_rejects_missing_directory_empty_and_unsupported_inputs() {
        let (_dir, library) = library();
        let source_dir = tempfile::TempDir::new().unwrap();
        let empty = source_dir.path().join("empty.mp4");
        let nonempty = source_dir.path().join("clip.mp4");
        fs::write(&empty, []).unwrap();
        fs::write(&nonempty, b"bytes").unwrap();

        for result in [
            import_video_file(
                &library,
                &source_dir.path().join("missing.mp4"),
                "video/mp4",
                "Missing",
            ),
            import_video_file(&library, source_dir.path(), "video/mp4", "Directory"),
            import_video_file(&library, &empty, "video/mp4", "Empty"),
            import_video_file(&library, &nonempty, "application/octet-stream", "Unknown"),
        ] {
            assert!(matches!(result, Err(SaveError::InvalidRequest(_))));
        }
        assert_staging_empty(&library);
    }

    #[test]
    fn video_staging_file_is_cleaned_when_the_save_fails() {
        let (_dir, library) = library();
        let source_dir = tempfile::TempDir::new().unwrap();
        let source = source_dir.path().join("clip.mp4");
        fs::write(&source, b"staged before lock failure").unwrap();
        // Blocking the lock directory makes the post-hash save fail after the
        // staging file exists, exercising the guard's downstream-error cleanup.
        fs::write(library.path().join(".cuttings-locks"), b"not a directory").unwrap();

        assert!(matches!(
            import_video_file(&library, &source, "video/mp4", "Clip"),
            Err(SaveError::Storage(_))
        ));
        assert_staging_empty(&library);
    }

    #[test]
    fn video_keeps_final_asset_when_note_or_article_commit_fails() {
        let source_dir = tempfile::TempDir::new().unwrap();

        let (_note_dir, note_library) = library();
        let note_bytes = b"video surviving note failure";
        let note_source = source_dir.path().join("note-failure.mp4");
        fs::write(&note_source, note_bytes).unwrap();
        let note_hash = sha256_hex(note_bytes);
        let note_id = local_video_id(&note_hash);
        fs::create_dir_all(note_library.note_path(&note_id)).unwrap();

        let note_result = import_video_file_with_options(
            &note_library,
            &note_source,
            "video/mp4",
            ImportOptions {
                state: ImportedReadingState {
                    note_markdown: Some("imported note".to_string()),
                    ..ImportedReadingState::default()
                },
                ..ImportOptions::default()
            },
        );

        assert!(matches!(note_result, Err(SaveError::Storage(_))));
        assert!(!note_library.article_path(&note_id).is_file());
        assert_eq!(
            fs::read(
                note_library
                    .assets_dir(&note_id)
                    .join(format!("{note_hash}.mp4"))
            )
            .unwrap(),
            note_bytes
        );

        let (_article_dir, article_library) = library();
        let article_bytes = b"video surviving article failure";
        let article_source = source_dir.path().join("article-failure.mp4");
        fs::write(&article_source, article_bytes).unwrap();
        let article_hash = sha256_hex(article_bytes);
        let article_id = local_video_id(&article_hash);
        fs::create_dir_all(article_library.article_path(&article_id)).unwrap();

        let article_result = import_video_file(
            &article_library,
            &article_source,
            "video/mp4",
            "Failed article",
        );

        assert!(matches!(article_result, Err(SaveError::Storage(_))));
        assert!(!article_library.article_path(&article_id).is_file());
        assert_eq!(
            fs::read(
                article_library
                    .assets_dir(&article_id)
                    .join(format!("{article_hash}.mp4"))
            )
            .unwrap(),
            article_bytes
        );
    }

    #[test]
    fn video_content_types_map_to_safe_canonical_extensions() {
        assert_eq!(video_extension("video/mp4").unwrap(), "mp4");
        assert_eq!(
            video_extension("VIDEO/QUICKTIME; charset=binary").unwrap(),
            "mov"
        );
        assert_eq!(video_extension("video/x-m4v").unwrap(), "m4v");
        assert_eq!(video_extension("video/webm").unwrap(), "webm");
        assert!(video_extension("../video/mp4").is_err());
        assert!(video_extension("application/octet-stream").is_err());
    }

    #[test]
    fn imports_reject_empty_or_non_http_content() {
        let (_dir, library) = library();
        assert!(matches!(
            import_link(&library, "file:///tmp/a"),
            Err(SaveError::InvalidRequest(_))
        ));
        assert!(matches!(
            import_text(&library, " \n\t", None),
            Err(SaveError::InvalidRequest(_))
        ));
        assert!(matches!(
            import_image(&library, vec![], "image/png", ""),
            Err(SaveError::InvalidRequest(_))
        ));
        assert!(matches!(
            import_image_from_origin_with_options(
                &library,
                b"bytes".to_vec(),
                "image/png",
                "file:///tmp/source-page",
                ImportOptions::default(),
            ),
            Err(SaveError::InvalidRequest(_))
        ));
    }

    #[test]
    fn concurrent_state_write_before_upgrade_preserves_state_and_full_body() {
        let (_dir, library) = library();
        let url = "https://example.com/concurrent-before";
        let imported = import_link(&library, url).unwrap();

        // Hold the exact sidecar the public upgrade path must acquire. Start the
        // upgrade before committing a state edit made from the lightweight
        // snapshot; it cannot read that snapshot until this atomic write ends.
        let lock = lock_reading(&library, &imported.id).unwrap();
        let library_path = library.path().to_path_buf();
        let (started_tx, started_rx) = mpsc::channel();
        let upgrade = std::thread::spawn(move || {
            let library = LibraryRoot::new(library_path).unwrap();
            started_tx.send(()).unwrap();
            save_capture(&library, full_capture(url)).unwrap()
        });
        started_rx.recv().unwrap();

        let content = std::fs::read_to_string(library.article_path(&imported.id)).unwrap();
        let mut reading = parse_reading(&content).unwrap();
        reading.metadata.tags.push("raced-tag".to_string());
        reading.metadata.favorite = true;
        reading.metadata.rating = 5;
        write_reading_under_lock(&library, reading.metadata, reading.body, &lock).unwrap();
        drop(lock);

        let outcome = upgrade.join().unwrap();
        assert_eq!(outcome.disposition, SaveDisposition::Upgraded);
        let reading =
            parse_reading(&std::fs::read_to_string(library.article_path(&imported.id)).unwrap())
                .unwrap();
        assert_eq!(reading.body, "# Captured title\n\nFull body.\n");
        assert_eq!(reading.metadata.tags, vec!["raced-tag"]);
        assert!(reading.metadata.favorite);
        assert_eq!(reading.metadata.rating, 5);
    }

    #[test]
    fn concurrent_app_mutations_after_upgrade_preserve_full_body() {
        let (_dir, library) = library();
        let url = "https://example.com/concurrent-after";
        let imported = import_link(&library, url).unwrap();
        let index_dir = tempfile::TempDir::new().unwrap();
        let index_path = index_dir.path().join("index.db");
        let conn = crate::open_index(&index_path).unwrap();
        crate::rebuild(&conn, &library).unwrap();
        drop(conn);

        // Keep the upgrade lock while the app-side tag/status/rating worker is
        // launched. Its real public RMW methods can proceed only after the full
        // body is atomically installed, so each reads and retains that body.
        let lock = lock_reading(&library, &imported.id).unwrap();
        let library_path = library.path().to_path_buf();
        let id = imported.id.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let mutations = std::thread::spawn(move || {
            let library = LibraryRoot::new(library_path).unwrap();
            let conn = crate::open_index(&index_path).unwrap();
            started_tx.send(()).unwrap();
            crate::add_tag(&library, &conn, &id, "after-upgrade").unwrap();
            set_read(&library, &conn, &id, true).unwrap();
            set_favorite(&library, &conn, &id, true).unwrap();
            set_rating(&library, &conn, &id, 4).unwrap();
        });
        started_rx.recv().unwrap();

        let outcome =
            save_capture_under_lock(&library, full_capture(url), imported.id.clone(), &lock)
                .unwrap();
        drop(lock);
        mutations.join().unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Upgraded);
        let reading =
            parse_reading(&std::fs::read_to_string(library.article_path(&imported.id)).unwrap())
                .unwrap();
        assert_eq!(reading.body, "# Captured title\n\nFull body.\n");
        assert_eq!(reading.metadata.tags, vec!["after-upgrade"]);
        assert!(reading.metadata.read_at.is_some());
        assert!(reading.metadata.favorite);
        assert_eq!(reading.metadata.rating, 4);
    }
}
