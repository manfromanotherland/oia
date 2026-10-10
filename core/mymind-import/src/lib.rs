// SPDX-License-Identifier: MIT

//! Offline adapter from mymind's exported `cards.csv` and media files into
//! Óia' shared Rust import pipeline.

mod link_metadata;
mod product_migration;

pub use category_migration::{
    enrich_existing_categories, EnrichExistingCategoriesOptions, EnrichExistingCategoriesReport,
};
pub use product_migration::{
    enrich_existing_products, EnrichExistingProductsOptions, EnrichExistingProductsReport,
};
mod category_migration;

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt::Write as _,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
};

use anyhow::{bail, Result};
use oia_core::{
    delete_unenriched_link_files_if_unchanged, import_image_from_origin_with_options,
    import_image_with_options, import_link_capture, import_link_capture_if_unchanged,
    import_link_with_options, import_reading, import_text_with_options,
    import_video_file_from_origin_with_options, import_video_file_with_options,
    normalize_theme_color, normalize_url, parse_reading, quote_id, sha256_hex, url_id,
    ImportOptions, ImportedReadingState, LibraryRoot, ReadingKind, SaveDisposition, SaveInput,
    SaveLinkInput, SaveOutcome, MAX_TAG_LEN,
};
use sha2::{Digest, Sha256};
use url::Url;

use link_metadata::{
    HttpLinkMetadataFetcher, LinkFetchError, LinkMetadataCapture, LinkMetadataFetcher,
};

const CSV_FILENAME: &str = "cards.csv";
const REQUIRED_HEADERS: [&str; 8] = [
    "id", "type", "title", "url", "content", "note", "tags", "created",
];
const QUOTE_EXCERPT_CHARACTERS: usize = 600;

const WARN_MISSING_DATE: &str = "rows without a usable creation date";
const WARN_LONG_TAG: &str = "overlong tags omitted";
const ERROR_CSV_RECORD: &str = "malformed CSV records";
const ERROR_NOTE_CONFLICT: &str = "duplicate readings with conflicting notes";
const ERROR_IMPORT: &str = "readings that failed while writing";
const ERROR_DELETE: &str = "dead links that could not be removed safely";
const WARN_UNREACHABLE_GUARD: &str =
    "unreachable links retained because failures looked network-wide";

const SKIP_DOCUMENT: &str = "documents (PDF is not a Óia card kind)";
const SKIP_EMPTY_QUOTATION: &str = "quotations without content";
const SKIP_PLACEHOLDER: &str = "placeholder rows";
const SKIP_SCREENSHOT: &str = "screenshots without an HTTP(S) source";
const SKIP_MEDIA_ID: &str = "media rows without one exact ID-to-filename match";
const SKIP_MEDIA_TYPE: &str = "media rows with an unsupported exported file type";
const SKIP_EMPTY_MEDIA: &str = "media rows with unreadable or empty files";
const SKIP_UNIMPORTABLE: &str = "rows without importable content or an HTTP(S) source";
const SKIP_UNAVAILABLE_LINK: &str = "web links unavailable during metadata fetch";
const SKIP_ORPHAN_MEDIA: &str = "exported files not referenced by a CSV row ID";
const SKIP_UNSUPPORTED_FILE: &str = "unsupported exported files";
const SKIP_SYMLINK: &str = "symbolic links";

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub export: PathBuf,
    pub library: PathBuf,
    /// `false` is a read-only preview; `true` writes through `oia-core`.
    pub write: bool,
    /// Include one line per reading, identified only by opaque IDs.
    pub verbose: bool,
    /// Fetch website metadata/assets for link rows before writing them.
    pub enrich_links: bool,
    /// Maximum concurrent link fetches. Each host is additionally capped at two.
    pub workers: usize,
}

#[derive(Debug, Clone)]
pub struct EnrichExistingOptions {
    pub library: PathBuf,
    /// `false` snapshots the target only; `true` fetches and persists metadata.
    pub write: bool,
    /// Include one line per reading, identified only by its opaque ID.
    pub verbose: bool,
    /// Maximum concurrent link fetches. Each host is additionally capped at two.
    pub workers: usize,
    /// Optional guard against applying to a changed target set.
    pub expected_count: Option<usize>,
    /// SHA-256 of the sorted, newline-delimited target IDs.
    pub expected_digest: Option<String>,
}

#[derive(Debug, Default)]
pub struct EnrichExistingReport {
    pub planned: usize,
    pub target_digest: String,
    pub upgraded: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub warnings: usize,
    pub errors: usize,
    pub link_fetches: usize,
    pub link_previews: usize,
    pub link_favicons: usize,
    pub link_theme_colors: usize,
    pub link_fetch_failures: usize,
    write: bool,
    warning_reasons: BTreeMap<&'static str, usize>,
    error_reasons: BTreeMap<&'static str, usize>,
    verbose_lines: Vec<String>,
}

impl EnrichExistingReport {
    pub fn render(&self) -> String {
        let mut rendered = String::new();
        rendered.push_str(if self.write {
            "link metadata migration\n"
        } else {
            "link metadata migration preview\n"
        });
        rendered.push_str(&format!("Target links: {}\n", self.planned));
        rendered.push_str(&format!("Target digest: {}\n", self.target_digest));
        render_reasons(&mut rendered, "Warnings", &self.warning_reasons);
        render_reasons(&mut rendered, "Errors", &self.error_reasons);
        if self.write {
            rendered.push_str(&format!(
                "Link metadata: fetched {}, previews {}, favicons {}, theme colors {}, unavailable {}, removed {}\n",
                self.link_fetches,
                self.link_previews,
                self.link_favicons,
                self.link_theme_colors,
                self.link_fetch_failures,
                self.removed
            ));
        } else if self.planned > 0 {
            rendered.push_str(&format!(
                "Link metadata fetches planned on write: {}\n",
                self.planned
            ));
        }
        if !self.verbose_lines.is_empty() {
            rendered.push('\n');
            for line in &self.verbose_lines {
                rendered.push_str(line);
                rendered.push('\n');
            }
        }
        rendered.push_str(&format!(
            "\nSummary: target links {}, enriched {}, removed {}, unchanged {}, warnings {}, errors {}\n",
            self.planned,
            self.upgraded,
            self.removed,
            self.unchanged,
            self.warnings,
            self.errors
        ));
        if !self.write {
            rendered.push_str(
                "No library files were written. Re-run with --write to apply this exact target.\n",
            );
        }
        rendered
    }
}

#[derive(Debug, Default)]
pub struct RunReport {
    pub source_rows: usize,
    pub planned: usize,
    pub merged_duplicates: usize,
    pub saved: usize,
    pub upgraded: usize,
    pub removed: usize,
    /// Readings already present in the destination library.
    pub duplicates: usize,
    pub skipped: usize,
    pub warnings: usize,
    pub errors: usize,
    pub link_fetches: usize,
    pub link_previews: usize,
    pub link_favicons: usize,
    pub link_theme_colors: usize,
    pub link_fetch_failures: usize,
    write: bool,
    enrich_links: bool,
    write_blocked: bool,
    planned_by_action: BTreeMap<&'static str, usize>,
    skip_reasons: BTreeMap<&'static str, usize>,
    warning_reasons: BTreeMap<&'static str, usize>,
    error_reasons: BTreeMap<&'static str, usize>,
    verbose_lines: Vec<String>,
}

impl RunReport {
    pub fn render(&self) -> String {
        let mut rendered = String::new();
        rendered.push_str(if self.write {
            "mymind import\n"
        } else {
            "mymind import preview\n"
        });
        rendered.push_str(&format!("Source rows: {}\n", self.source_rows));
        rendered.push_str(&format!("Planned unique readings: {}\n", self.planned));
        rendered.push_str(&format!(
            "Merged duplicate rows: {}\n",
            self.merged_duplicates
        ));
        if !self.planned_by_action.is_empty() {
            rendered.push_str("Plan:");
            for (action, count) in &self.planned_by_action {
                rendered.push_str(&format!(" {action} {count}"));
            }
            rendered.push('\n');
        }
        render_reasons(&mut rendered, "Skipped", &self.skip_reasons);
        render_reasons(&mut rendered, "Warnings", &self.warning_reasons);
        render_reasons(&mut rendered, "Errors", &self.error_reasons);
        if self.link_fetches > 0 || self.link_fetch_failures > 0 {
            rendered.push_str(&format!(
                "Link metadata: fetched {}, previews {}, favicons {}, theme colors {}, unavailable {}, removed {}\n",
                self.link_fetches,
                self.link_previews,
                self.link_favicons,
                self.link_theme_colors,
                self.link_fetch_failures,
                self.removed
            ));
        } else if self.enrich_links
            && !self.write
            && self.planned_by_action.get("articles").copied().unwrap_or(0) > 0
        {
            rendered.push_str(&format!(
                "Link metadata fetches planned on write: {}\n",
                self.planned_by_action.get("articles").copied().unwrap_or(0)
            ));
        }
        if !self.verbose_lines.is_empty() {
            rendered.push('\n');
            for line in &self.verbose_lines {
                rendered.push_str(line);
                rendered.push('\n');
            }
        }
        rendered.push_str(&format!(
            "\nSummary: source rows {}, planned unique readings {}, merged duplicate rows {}, saved {}, upgraded {}, removed {}, already present {}, skipped {}, warnings {}, errors {}\n",
            self.source_rows,
            self.planned,
            self.merged_duplicates,
            self.saved,
            self.upgraded,
            self.removed,
            self.duplicates,
            self.skipped,
            self.warnings,
            self.errors
        ));
        if self.write_blocked {
            rendered.push_str(
                "No library files were written because planning found blocking errors.\n",
            );
        } else if !self.write {
            rendered.push_str(
                "No library files were written. Re-run with --write to import this plan.\n",
            );
        }
        rendered
    }

    fn add_error(&mut self, reason: &'static str) {
        *self.error_reasons.entry(reason).or_default() += 1;
        self.errors += 1;
    }

    fn add_warning(&mut self, reason: &'static str) {
        *self.warning_reasons.entry(reason).or_default() += 1;
        self.warnings += 1;
    }
}

fn render_reasons(rendered: &mut String, heading: &str, reasons: &BTreeMap<&'static str, usize>) {
    if reasons.is_empty() {
        return;
    }
    rendered.push_str(heading);
    rendered.push_str(":\n");
    for (reason, count) in reasons {
        rendered.push_str(&format!("  {count} {reason}\n"));
    }
}

#[derive(Debug, Clone, Default)]
struct MymindRow {
    id: String,
    card_type: String,
    title: String,
    url: String,
    content: String,
    note: String,
    tags: String,
    created: String,
}

impl MymindRow {
    fn from_record(record: &csv::StringRecord, headers: &HeaderIndexes) -> Self {
        let value = |index: usize| record.get(index).unwrap_or_default().to_string();
        Self {
            id: value(headers.id),
            card_type: value(headers.card_type),
            title: value(headers.title),
            url: value(headers.url),
            content: value(headers.content),
            note: value(headers.note),
            tags: value(headers.tags),
            created: value(headers.created),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct HeaderIndexes {
    id: usize,
    card_type: usize,
    title: usize,
    url: usize,
    content: usize,
    note: usize,
    tags: usize,
    created: usize,
}

#[derive(Debug, Default)]
struct Diagnostics {
    skips: BTreeMap<&'static str, usize>,
    warnings: BTreeMap<&'static str, usize>,
    errors: BTreeMap<&'static str, usize>,
}

impl Diagnostics {
    fn skip(&mut self, reason: &'static str) {
        *self.skips.entry(reason).or_default() += 1;
    }

    fn warn(&mut self, reason: &'static str) {
        *self.warnings.entry(reason).or_default() += 1;
    }

    fn error(&mut self, reason: &'static str) {
        *self.errors.entry(reason).or_default() += 1;
    }
}

#[derive(Debug)]
struct ImportPlan {
    items: Vec<PlannedItem>,
    source_rows: usize,
    merged_duplicates: usize,
    diagnostics: Diagnostics,
}

#[derive(Debug)]
struct PlannedItem {
    label: String,
    identity: String,
    action: PlannedAction,
    title: Option<String>,
    saved_at: Option<String>,
    state: ImportedReadingState,
    /// Exact `article.md` bytes observed by an existing-library plan. Export
    /// imports leave this unset because they are not mutating a prior snapshot.
    expected_article_sha256: Option<String>,
    /// `None` means an existing-library plan observed no `note.md`. This is
    /// consulted only before deletion; metadata enrichment preserves notes.
    expected_note_sha256: Option<String>,
}

impl PlannedItem {
    fn merge(&mut self, newer: PlannedItem) -> bool {
        if newer.title.is_some() {
            self.title = newer.title;
        }
        self.saved_at = earliest_date(self.saved_at.take(), newer.saved_at);
        for tag in newer.state.tags {
            if !self.state.tags.contains(&tag) {
                self.state.tags.push(tag);
            }
        }
        let existing_note = self.state.note_markdown.take();
        match (existing_note, newer.state.note_markdown) {
            (Some(existing), Some(incoming)) if existing != incoming => {
                self.state.note_markdown = Some(existing);
                false
            }
            (Some(existing), _) => {
                self.state.note_markdown = Some(existing);
                true
            }
            (None, Some(incoming)) => {
                self.state.note_markdown = Some(incoming);
                true
            }
            _ => true,
        }
    }
}

#[derive(Debug)]
enum PlannedAction {
    Link {
        url: String,
    },
    Quote {
        origin: Option<String>,
        content: String,
    },
    Image {
        file: PathBuf,
        origin: Option<String>,
        content_type: &'static str,
        content_hash: String,
    },
    Video {
        file: PathBuf,
        origin: Option<String>,
        content_type: &'static str,
        content_hash: String,
    },
}

impl PlannedAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Link { .. } => "articles",
            Self::Quote { .. } => "quotes",
            Self::Image { .. } => "images",
            Self::Video { .. } => "videos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportedFileKind {
    Image,
    Video,
    Pdf,
    Other,
}

#[derive(Debug)]
struct ExportedFile {
    path: PathBuf,
    kind: ExportedFileKind,
    content_type: Option<&'static str>,
    referenced: bool,
    content_hash: Option<String>,
}

struct MediaInventory {
    files: Vec<ExportedFile>,
    by_stem: HashMap<String, Vec<usize>>,
}

pub fn run(options: RunOptions) -> Result<RunReport> {
    run_with_fetcher(options, None)
}

pub fn enrich_existing_links(options: EnrichExistingOptions) -> Result<EnrichExistingReport> {
    enrich_existing_links_with_fetcher(options, None)
}

fn enrich_existing_links_with_fetcher(
    options: EnrichExistingOptions,
    injected_fetcher: Option<Arc<dyn LinkMetadataFetcher>>,
) -> Result<EnrichExistingReport> {
    if !options.library.is_dir() {
        bail!("Óia library is not a directory");
    }
    let library_path = options
        .library
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("could not resolve the library folder"))?;
    let library =
        LibraryRoot::new(&library_path).map_err(|_| anyhow::anyhow!("invalid library folder"))?;
    let items = existing_link_plan(&library)?;
    let target_digest = target_digest(&items);

    if let Some(expected) = options.expected_count {
        if expected != items.len() {
            bail!(
                "target snapshot changed: expected {expected} links but found {}",
                items.len()
            );
        }
    }
    if let Some(expected) = options.expected_digest.as_deref() {
        let expected = expected.trim().to_ascii_lowercase();
        if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("expected target digest must be exactly 64 hexadecimal characters");
        }
        if expected != target_digest {
            bail!("target snapshot changed: digest does not match");
        }
    }

    let mut execution = RunReport {
        write: options.write,
        enrich_links: true,
        planned: items.len(),
        ..RunReport::default()
    };
    if options.write && !items.is_empty() {
        let fetcher = match injected_fetcher {
            Some(fetcher) => fetcher,
            None => Arc::new(HttpLinkMetadataFetcher::new()?),
        };
        execute_enriched_links(
            &library_path,
            items,
            fetcher,
            options.workers,
            options.verbose,
            true,
            &mut execution,
        );
    }

    Ok(EnrichExistingReport {
        planned: execution.planned,
        target_digest,
        upgraded: execution.upgraded,
        removed: execution.removed,
        unchanged: execution.duplicates,
        warnings: execution.warnings,
        errors: execution.errors,
        link_fetches: execution.link_fetches,
        link_previews: execution.link_previews,
        link_favicons: execution.link_favicons,
        link_theme_colors: execution.link_theme_colors,
        link_fetch_failures: execution.link_fetch_failures,
        write: options.write,
        warning_reasons: execution.warning_reasons,
        error_reasons: execution.error_reasons,
        verbose_lines: execution.verbose_lines,
    })
}

fn existing_link_plan(library: &LibraryRoot) -> Result<Vec<PlannedItem>> {
    let mut items = Vec::new();
    let articles = library.articles_dir();
    if !articles.is_dir() {
        return Ok(items);
    }
    for bucket in fs::read_dir(&articles)? {
        let bucket = bucket?;
        if !bucket.file_type()?.is_dir() {
            continue;
        }
        for reading_directory in fs::read_dir(bucket.path())? {
            let reading_directory = reading_directory?;
            if !reading_directory.file_type()?.is_dir() {
                continue;
            }
            let article_path = reading_directory.path().join("article.md");
            let Ok(article_bytes) = fs::read(&article_path) else {
                continue;
            };
            let Ok(article_text) = std::str::from_utf8(&article_bytes) else {
                continue;
            };
            let Ok(reading) = parse_reading(article_text) else {
                continue;
            };
            let metadata = reading.metadata;
            if reading_directory.path() != library.reading_dir(&metadata.id) {
                continue;
            }
            if metadata.kind != ReadingKind::Article
                || !metadata.lightweight
                || metadata.preview_asset.is_some()
                || metadata.favicon_asset.is_some()
            {
                continue;
            }
            let Some(url) = http_url(&metadata.url) else {
                continue;
            };
            if url_id(&url).ok().as_deref() != Some(metadata.id.as_str()) {
                continue;
            }
            let expected_note_sha256 = match fs::read(library.note_path(&metadata.id)) {
                Ok(note_bytes) => Some(sha256_hex(&note_bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => continue,
            };
            items.push(PlannedItem {
                label: format!("reading {}", metadata.id),
                identity: format!("article:{}", metadata.id),
                action: PlannedAction::Link { url },
                title: Some(metadata.title),
                saved_at: Some(metadata.saved_at),
                state: ImportedReadingState::default(),
                expected_article_sha256: Some(sha256_hex(&article_bytes)),
                expected_note_sha256,
            });
        }
    }
    items.sort_by(|left, right| left.identity.cmp(&right.identity));
    Ok(items)
}

fn target_digest(items: &[PlannedItem]) -> String {
    let mut ids = items
        .iter()
        .map(|item| {
            item.identity
                .strip_prefix("article:")
                .unwrap_or(&item.identity)
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !ids.is_empty() {
        ids.push('\n');
    }
    sha256_hex(ids.as_bytes())
}

fn run_with_fetcher(
    options: RunOptions,
    injected_fetcher: Option<Arc<dyn LinkMetadataFetcher>>,
) -> Result<RunReport> {
    let (export_root, library_root) = validate_roots(&options.export, &options.library)?;
    let library =
        LibraryRoot::new(&library_root).map_err(|_| anyhow::anyhow!("invalid library folder"))?;
    let plan = build_plan(&export_root)?;
    let mut report = RunReport {
        source_rows: plan.source_rows,
        planned: plan.items.len(),
        merged_duplicates: plan.merged_duplicates,
        skipped: plan.diagnostics.skips.values().sum(),
        warnings: plan.diagnostics.warnings.values().sum(),
        errors: plan.diagnostics.errors.values().sum(),
        write: options.write,
        enrich_links: options.enrich_links,
        planned_by_action: count_actions(&plan.items),
        skip_reasons: plan.diagnostics.skips,
        warning_reasons: plan.diagnostics.warnings,
        error_reasons: plan.diagnostics.errors,
        ..RunReport::default()
    };
    if options.write && report.errors > 0 {
        report.write_blocked = true;
        return Ok(report);
    }

    let mut link_items = Vec::new();
    let mut ordinary_items = Vec::new();
    for item in plan.items {
        if options.write
            && options.enrich_links
            && matches!(&item.action, PlannedAction::Link { .. })
        {
            link_items.push(item);
        } else {
            ordinary_items.push(item);
        }
    }

    for item in ordinary_items {
        let action_label = item.action.label();
        if !options.write {
            if options.verbose {
                report
                    .verbose_lines
                    .push(format!("PLAN  {}: {action_label}", item.label));
            }
            continue;
        }
        match execute(&library, item) {
            Ok((label, outcome)) => {
                record_outcome(&mut report, &label, action_label, outcome, options.verbose)
            }
            Err(label) => {
                report.add_error(ERROR_IMPORT);
                if options.verbose {
                    report
                        .verbose_lines
                        .push(format!("ERROR {label}: import failed"));
                }
            }
        }
    }

    if options.write && options.enrich_links && !link_items.is_empty() {
        let fetcher = match injected_fetcher {
            Some(fetcher) => fetcher,
            None => Arc::new(HttpLinkMetadataFetcher::new()?),
        };
        execute_enriched_links(
            &library_root,
            link_items,
            fetcher,
            options.workers,
            options.verbose,
            false,
            &mut report,
        );
    }
    Ok(report)
}

fn count_actions(items: &[PlannedItem]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for item in items {
        *counts.entry(item.action.label()).or_default() += 1;
    }
    counts
}

fn validate_roots(export: &Path, library: &Path) -> Result<(PathBuf, PathBuf)> {
    if !export.is_dir() {
        bail!("mymind export is not a directory");
    }
    if !library.is_dir() {
        bail!("Óia library is not a directory");
    }
    let requested_export = export
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("could not resolve the export folder"))?;
    let library = library
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("could not resolve the library folder"))?;
    if requested_export == library
        || requested_export.starts_with(&library)
        || library.starts_with(&requested_export)
    {
        bail!("export and library folders must be separate and must not contain one another");
    }
    Ok((resolve_export_root(&requested_export)?, library))
}

fn resolve_export_root(requested: &Path) -> Result<PathBuf> {
    if requested.join(CSV_FILENAME).is_file() {
        return Ok(requested.to_path_buf());
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(requested)
        .map_err(|_| anyhow::anyhow!("could not inspect the export folder"))?
    {
        let entry = entry.map_err(|_| anyhow::anyhow!("could not inspect the export folder"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| anyhow::anyhow!("could not inspect the export folder"))?;
        if metadata.is_dir() && path.join(CSV_FILENAME).is_file() {
            candidates.push(path);
        }
    }
    match candidates.as_slice() {
        [candidate] => candidate
            .canonicalize()
            .map_err(|_| anyhow::anyhow!("could not resolve the nested export folder")),
        [] => bail!("the export folder does not contain cards.csv directly or in one immediate child folder"),
        _ => bail!("more than one immediate child folder contains cards.csv"),
    }
}

fn build_plan(export_root: &Path) -> Result<ImportPlan> {
    let csv_path = export_root.join(CSV_FILENAME);
    let mut diagnostics = Diagnostics::default();
    let MediaInventory {
        mut files,
        by_stem: files_by_stem,
    } = inventory_files(export_root, &csv_path, &mut diagnostics)?;
    let mut groups: Vec<(PlannedItem, bool)> = Vec::new();
    let mut group_by_identity = HashMap::<String, usize>::new();
    let mut source_rows = 0;
    let mut merged_duplicates = 0;
    let mut reader = csv::ReaderBuilder::new()
        .from_path(&csv_path)
        .map_err(|_| anyhow::anyhow!("could not open cards.csv"))?;
    let headers = validate_headers(
        reader
            .headers()
            .map_err(|_| anyhow::anyhow!("could not read cards.csv headers"))?,
    )?;

    for record in reader.records() {
        source_rows += 1;
        let record = match record {
            Ok(record) => record,
            Err(_) => {
                diagnostics.error(ERROR_CSV_RECORD);
                continue;
            }
        };
        let row = MymindRow::from_record(&record, &headers);
        let label = opaque_row_label(&row.id, source_rows);
        let Some(item) = plan_row(row, label, &mut files, &files_by_stem, &mut diagnostics) else {
            continue;
        };
        if let Some(index) = group_by_identity.get(&item.identity).copied() {
            merged_duplicates += 1;
            let (existing, blocked) = &mut groups[index];
            if !existing.merge(item) && !*blocked {
                diagnostics.error(ERROR_NOTE_CONFLICT);
                *blocked = true;
            }
        } else {
            group_by_identity.insert(item.identity.clone(), groups.len());
            groups.push((item, false));
        }
    }
    for file in files.iter().filter(|file| !file.referenced) {
        diagnostics.skip(match file.kind {
            ExportedFileKind::Image | ExportedFileKind::Video | ExportedFileKind::Pdf => {
                SKIP_ORPHAN_MEDIA
            }
            ExportedFileKind::Other => SKIP_UNSUPPORTED_FILE,
        });
    }
    let items = groups
        .into_iter()
        .filter_map(|(item, blocked)| (!blocked).then_some(item))
        .collect();
    Ok(ImportPlan {
        items,
        source_rows,
        merged_duplicates,
        diagnostics,
    })
}

fn validate_headers(headers: &csv::StringRecord) -> Result<HeaderIndexes> {
    let header = |index: usize| {
        headers
            .get(index)
            .unwrap_or_default()
            .trim_start_matches('\u{feff}')
            .trim()
    };
    let find = |required: &str| {
        headers
            .iter()
            .enumerate()
            .filter(|(index, _)| header(*index) == required)
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };
    let mut positions = HashMap::new();
    for required in REQUIRED_HEADERS {
        let matches = find(required);
        if matches.len() != 1 {
            bail!("cards.csv must contain each required column exactly once");
        }
        positions.insert(required, matches[0]);
    }
    Ok(HeaderIndexes {
        id: positions["id"],
        card_type: positions["type"],
        title: positions["title"],
        url: positions["url"],
        content: positions["content"],
        note: positions["note"],
        tags: positions["tags"],
        created: positions["created"],
    })
}

fn inventory_files(
    export_root: &Path,
    csv_path: &Path,
    diagnostics: &mut Diagnostics,
) -> Result<MediaInventory> {
    let canonical_csv = csv_path
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("could not resolve cards.csv"))?;
    let mut paths = Vec::new();
    collect_files(export_root, &mut paths, diagnostics)?;
    paths.sort();
    let mut files = Vec::new();
    let mut files_by_stem = HashMap::<String, Vec<usize>>::new();
    for path in paths {
        let canonical = match path.canonicalize() {
            Ok(canonical) if canonical.starts_with(export_root) => canonical,
            _ => {
                diagnostics.skip(SKIP_SYMLINK);
                continue;
            }
        };
        if canonical == canonical_csv || path.file_name().is_some_and(|name| name == ".DS_Store") {
            continue;
        }
        let (kind, content_type) = classify_file(&path);
        let index = files.len();
        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            files_by_stem
                .entry(stem.to_string())
                .or_default()
                .push(index);
        }
        files.push(ExportedFile {
            path,
            kind,
            content_type,
            referenced: false,
            content_hash: None,
        });
    }
    Ok(MediaInventory {
        files,
        by_stem: files_by_stem,
    })
}

fn collect_files(
    directory: &Path,
    paths: &mut Vec<PathBuf>,
    diagnostics: &mut Diagnostics,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .map_err(|_| anyhow::anyhow!("could not read the export folder"))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|_| anyhow::anyhow!("could not read the export folder"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| anyhow::anyhow!("could not inspect an exported file"))?;
        if metadata.file_type().is_symlink() {
            diagnostics.skip(SKIP_SYMLINK);
        } else if metadata.is_dir() {
            collect_files(&path, paths, diagnostics)?;
        } else if metadata.is_file() {
            paths.push(path);
        }
    }
    Ok(())
}

fn plan_row(
    row: MymindRow,
    label: String,
    files: &mut [ExportedFile],
    files_by_stem: &HashMap<String, Vec<usize>>,
    diagnostics: &mut Diagnostics,
) -> Option<PlannedItem> {
    let title = nonempty(&row.title).map(str::to_string);
    let saved_at = normalize_created(&row.created);
    if saved_at.is_none() {
        diagnostics.warn(WARN_MISSING_DATE);
    }
    let state = ImportedReadingState {
        favorite: false,
        tags: normalize_tags(&row.tags, diagnostics),
        note_markdown: normalize_note(&row.note),
    };
    let card_type = row.card_type.trim().to_ascii_lowercase();
    let origin = http_url(&row.url);
    let action = match card_type.as_str() {
        "image" => plan_media(
            &row.id,
            ExportedFileKind::Image,
            origin,
            files,
            files_by_stem,
            diagnostics,
        )?,
        "video" => plan_media(
            &row.id,
            ExportedFileKind::Video,
            origin,
            files,
            files_by_stem,
            diagnostics,
        )?,
        "document" => {
            mark_matching_files(&row.id, files, files_by_stem);
            diagnostics.skip(SKIP_DOCUMENT);
            if let Some(url) = origin {
                PlannedAction::Link { url }
            } else {
                return None;
            }
        }
        "placeholder" => {
            diagnostics.skip(SKIP_PLACEHOLDER);
            return None;
        }
        "note" => {
            let Some(content) = normalized_content(&row.content) else {
                diagnostics.skip(SKIP_UNIMPORTABLE);
                return None;
            };
            PlannedAction::Quote {
                origin: None,
                content,
            }
        }
        "content" => {
            if let Some(content) = normalized_content(&row.content) {
                PlannedAction::Quote { origin, content }
            } else if let Some(url) = origin {
                PlannedAction::Link { url }
            } else {
                diagnostics.skip(SKIP_UNIMPORTABLE);
                return None;
            }
        }
        "quotation" => {
            if let Some(content) = normalized_content(&row.content) {
                PlannedAction::Quote { origin, content }
            } else if let Some(url) = origin {
                // Some exports omit the selected text but retain its source.
                // Preserve that last useful datum without pretending it is a
                // recoverable quote.
                PlannedAction::Link { url }
            } else {
                diagnostics.skip(SKIP_EMPTY_QUOTATION);
                return None;
            }
        }
        "screenshot" if origin.is_none() => {
            diagnostics.skip(SKIP_SCREENSHOT);
            return None;
        }
        _ => {
            let Some(url) = origin else {
                diagnostics.skip(SKIP_UNIMPORTABLE);
                return None;
            };
            PlannedAction::Link { url }
        }
    };
    let identity = action_identity(&action).ok()?;
    Some(PlannedItem {
        label,
        identity,
        action,
        title,
        saved_at,
        state,
        expected_article_sha256: None,
        expected_note_sha256: None,
    })
}

fn plan_media(
    row_id: &str,
    expected_kind: ExportedFileKind,
    origin: Option<String>,
    files: &mut [ExportedFile],
    files_by_stem: &HashMap<String, Vec<usize>>,
    diagnostics: &mut Diagnostics,
) -> Option<PlannedAction> {
    let indices = files_by_stem
        .get(row_id.trim())
        .cloned()
        .unwrap_or_default();
    for index in &indices {
        files[*index].referenced = true;
    }
    let [index] = indices.as_slice() else {
        diagnostics.skip(SKIP_MEDIA_ID);
        return None;
    };
    let file = &mut files[*index];
    if file.kind != expected_kind {
        diagnostics.skip(SKIP_MEDIA_TYPE);
        return None;
    }
    let content_type = match file.content_type {
        Some(content_type) => content_type,
        None => {
            diagnostics.skip(SKIP_MEDIA_TYPE);
            return None;
        }
    };
    let content_hash = if let Some(hash) = &file.content_hash {
        hash.clone()
    } else {
        let hash = match hash_nonempty_file(&file.path) {
            Some(hash) => hash,
            None => {
                diagnostics.skip(SKIP_EMPTY_MEDIA);
                return None;
            }
        };
        file.content_hash = Some(hash.clone());
        hash
    };
    Some(match expected_kind {
        ExportedFileKind::Image => PlannedAction::Image {
            file: file.path.clone(),
            origin,
            content_type,
            content_hash,
        },
        ExportedFileKind::Video => PlannedAction::Video {
            file: file.path.clone(),
            origin,
            content_type,
            content_hash,
        },
        ExportedFileKind::Pdf | ExportedFileKind::Other => unreachable!(),
    })
}

fn mark_matching_files(
    row_id: &str,
    files: &mut [ExportedFile],
    files_by_stem: &HashMap<String, Vec<usize>>,
) {
    if let Some(indices) = files_by_stem.get(row_id.trim()) {
        for index in indices {
            files[*index].referenced = true;
        }
    }
}

fn action_identity(action: &PlannedAction) -> Result<String> {
    match action {
        PlannedAction::Link { url } => Ok(format!("article:{}", url_id(url)?)),
        PlannedAction::Quote {
            origin: Some(origin),
            content,
        } => Ok(format!(
            "quote:{}",
            quote_id(origin, &quote_markdown(content))?
        )),
        PlannedAction::Quote {
            origin: None,
            content,
        } => {
            let identity_text = normalized_identity_text(content);
            let content_hash = sha256_hex(format!("quote\0{identity_text}").as_bytes());
            let local_origin = format!("cuttings://local/quote/{content_hash}");
            Ok(format!(
                "quote:{}",
                quote_id(&local_origin, &identity_text)?
            ))
        }
        PlannedAction::Image {
            origin,
            content_hash,
            ..
        } => Ok(media_identity("image", origin.as_deref(), content_hash)),
        PlannedAction::Video {
            origin,
            content_hash,
            ..
        } => Ok(media_identity("video", origin.as_deref(), content_hash)),
    }
}

fn media_identity(kind: &str, origin: Option<&str>, content_hash: &str) -> String {
    let origin = origin
        .map(str::to_string)
        .unwrap_or_else(|| format!("cuttings://local/{kind}/{content_hash}"));
    sha256_hex(format!("{kind}\0{origin}\0{content_hash}").as_bytes())
}

fn classify_file(path: &Path) -> (ExportedFileKind, Option<&'static str>) {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let image = match extension.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "avif" => Some("image/avif"),
        "heic" => Some("image/heic"),
        "heif" => Some("image/heif"),
        "tif" | "tiff" => Some("image/tiff"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/vnd.microsoft.icon"),
        "jp2" => Some("image/jp2"),
        "jxl" => Some("image/jxl"),
        _ => None,
    };
    if let Some(content_type) = image {
        return (ExportedFileKind::Image, Some(content_type));
    }
    if matches!(extension.as_str(), "blob" | "mov" | "mkv" | "m4v") {
        return sniff_video(path)
            .map(|content_type| (ExportedFileKind::Video, Some(content_type)))
            .unwrap_or((ExportedFileKind::Other, None));
    }
    let video = match extension.as_str() {
        "mp4" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mpeg" | "mpg" => Some("video/mpeg"),
        "avi" => Some("video/x-msvideo"),
        "ogv" | "ogg" => Some("video/ogg"),
        "3gp" => Some("video/3gpp"),
        "3g2" => Some("video/3gpp2"),
        _ => None,
    };
    if let Some(content_type) = video {
        return (ExportedFileKind::Video, Some(content_type));
    }
    if extension == "pdf" {
        (ExportedFileKind::Pdf, Some("application/pdf"))
    } else {
        (ExportedFileKind::Other, None)
    }
}

fn sniff_video(path: &Path) -> Option<&'static str> {
    let mut file = fs::File::open(path).ok()?;
    let mut prefix = [0_u8; 4096];
    let read = file.read(&mut prefix).ok()?;
    let bytes = &prefix[..read];
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let major = &bytes[8..12];
        if major == b"qt  " {
            return Some("video/quicktime");
        }
        if major.starts_with(b"M4V") {
            return Some("video/m4v");
        }
        return Some("video/mp4");
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        if contains_ascii_case_insensitive(bytes, b"webm") {
            return Some("video/webm");
        }
        if contains_ascii_case_insensitive(bytes, b"matroska")
            || path.extension().is_some_and(|extension| extension == "mkv")
        {
            return Some("video/x-matroska");
        }
    }
    None
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

#[derive(Debug, Default)]
struct LinkCaptureStats {
    preview: bool,
    favicon: bool,
    theme_color: bool,
}

struct EnrichedExecution {
    label: String,
    outcome: Option<std::result::Result<SaveOutcome, ()>>,
    fetch_error: Option<LinkFetchError>,
    removal: Option<RemovalCandidate>,
    stats: LinkCaptureStats,
}

struct RemovalCandidate {
    id: String,
    url: String,
    expected_article_sha256: String,
    expected_note_sha256: Option<String>,
    reason: LinkFetchError,
}

fn execute_enriched_links(
    library_path: &Path,
    items: Vec<PlannedItem>,
    fetcher: Arc<dyn LinkMetadataFetcher>,
    workers: usize,
    verbose: bool,
    delete_dead: bool,
    report: &mut RunReport,
) {
    let total = items.len();
    let worker_count = workers.clamp(1, 32).min(total.max(1));
    let queue = Arc::new(Mutex::new(VecDeque::from(items)));
    let (sender, receiver) = mpsc::channel();
    let mut reached_server = 0;
    let mut removals = Vec::new();

    thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let fetcher = Arc::clone(&fetcher);
            let sender = sender.clone();
            let library_path = library_path.to_path_buf();
            scope.spawn(move || {
                let library = LibraryRoot::new(&library_path)
                    .expect("the validated library remains available to link workers");
                loop {
                    let item = queue.lock().expect("link queue lock poisoned").pop_front();
                    let Some(item) = item else { break };
                    let execution = execute_enriched_link(&library, item, fetcher.as_ref());
                    if sender.send(execution).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        for (index, execution) in receiver.into_iter().enumerate() {
            let EnrichedExecution {
                label,
                outcome,
                fetch_error,
                removal,
                stats,
            } = execution;
            if let Some(error) = fetch_error {
                report.link_fetch_failures += 1;
                report.add_warning(error.description());
                reached_server += usize::from(error.reached_server());
            } else {
                reached_server += 1;
                report.link_fetches += 1;
                report.link_previews += usize::from(stats.preview);
                report.link_favicons += usize::from(stats.favicon);
                report.link_theme_colors += usize::from(stats.theme_color);
            }
            let pending_removal = removal.is_some();
            if let Some(removal) = removal {
                removals.push(removal);
            }
            match outcome {
                Some(Ok(outcome)) => record_outcome(report, &label, "articles", outcome, verbose),
                Some(Err(())) => {
                    report.add_error(ERROR_IMPORT);
                    if verbose {
                        report
                            .verbose_lines
                            .push(format!("ERROR {}: link metadata import failed", label));
                    }
                }
                None if !delete_dead => {
                    *report
                        .skip_reasons
                        .entry(SKIP_UNAVAILABLE_LINK)
                        .or_default() += 1;
                    report.skipped += 1;
                    if verbose {
                        report
                            .verbose_lines
                            .push(format!("SKIP    {label}: link unavailable"));
                    }
                }
                None if !pending_removal => {
                    report.duplicates += 1;
                    if verbose {
                        report.verbose_lines.push(format!(
                            "PRESENT {label}: changed while metadata was fetched"
                        ));
                    }
                }
                None if verbose => report
                    .verbose_lines
                    .push(format!("DEAD    {label}: pending safe removal")),
                None => {}
            }

            let processed = index + 1;
            if total >= 50 && (processed % 50 == 0 || processed == total) {
                eprintln!("Link metadata: processed {processed}/{total}");
            }
        }
    });

    if delete_dead && !removals.is_empty() {
        apply_dead_link_removals(
            library_path,
            total,
            reached_server,
            removals,
            verbose,
            report,
        );
    }
}

fn apply_dead_link_removals(
    library_path: &Path,
    total: usize,
    reached_server: usize,
    removals: Vec<RemovalCandidate>,
    verbose: bool,
    report: &mut RunReport,
) {
    let (gone, unreachable): (Vec<_>, Vec<_>) = removals
        .into_iter()
        .partition(|candidate| candidate.reason == LinkFetchError::Gone);
    let systemic_unreachable = !unreachable.is_empty()
        && (reached_server == 0 || (total >= 20 && unreachable.len().saturating_mul(4) > total));
    let mut approved = gone;
    if systemic_unreachable {
        *report
            .warning_reasons
            .entry(WARN_UNREACHABLE_GUARD)
            .or_default() += unreachable.len();
        report.warnings += unreachable.len();
        report.duplicates += unreachable.len();
        if verbose {
            for candidate in &unreachable {
                report.verbose_lines.push(format!(
                    "PRESENT reading {}: retained by network-wide failure guard",
                    candidate.id
                ));
            }
        }
    } else {
        approved.extend(unreachable);
    }

    let library = match LibraryRoot::new(library_path) {
        Ok(library) => library,
        Err(_) => {
            for _ in approved {
                report.add_error(ERROR_DELETE);
            }
            return;
        }
    };
    for candidate in approved {
        match delete_unenriched_link_files_if_unchanged(
            &library,
            &candidate.id,
            &candidate.url,
            &candidate.expected_article_sha256,
            candidate.expected_note_sha256.as_deref(),
        ) {
            Ok(true) => {
                report.removed += 1;
                if verbose {
                    report.verbose_lines.push(format!(
                        "REMOVED reading {}: {}",
                        candidate.id,
                        candidate.reason.description()
                    ));
                }
            }
            Ok(false) => {
                report.duplicates += 1;
                if verbose {
                    report.verbose_lines.push(format!(
                        "PRESENT reading {}: changed while metadata was fetched",
                        candidate.id
                    ));
                }
            }
            Err(_) => report.add_error(ERROR_DELETE),
        }
    }
}

fn execute_enriched_link(
    library: &LibraryRoot,
    item: PlannedItem,
    fetcher: &dyn LinkMetadataFetcher,
) -> EnrichedExecution {
    let PlannedItem {
        label,
        identity,
        action,
        title,
        saved_at,
        state,
        expected_article_sha256,
        expected_note_sha256,
    } = item;
    let PlannedAction::Link { url } = action else {
        return EnrichedExecution {
            label,
            outcome: Some(Err(())),
            fetch_error: None,
            removal: None,
            stats: LinkCaptureStats::default(),
        };
    };

    match fetcher.fetch(&url) {
        Ok(capture) => {
            let LinkMetadataCapture {
                canonical_url,
                title: captured_title,
                site,
                author,
                lang,
                excerpt,
                theme_color,
                images,
                preview_url,
                favicon_url,
                product,
                link_category,
            } = capture;
            let stats = LinkCaptureStats {
                preview: preview_url.is_some(),
                favicon: favicon_url.is_some(),
                theme_color: theme_color
                    .as_deref()
                    .and_then(normalize_theme_color)
                    .is_some(),
            };
            let input = SaveLinkInput {
                product,
                link_category,
                url,
                canonical_url,
                title: captured_title.or(title).unwrap_or_default(),
                author,
                site,
                saved_at: saved_at.unwrap_or_default(),
                images,
                preview_url,
                favicon_url,
                theme_color,
                excerpt,
                lang,
            };
            let outcome = if let Some(expected) = expected_article_sha256.as_deref() {
                import_link_capture_if_unchanged(library, input, state, expected)
            } else {
                import_link_capture(library, input, state)
            };
            EnrichedExecution {
                label,
                outcome: Some(outcome.map_err(|_| ())),
                fetch_error: None,
                removal: None,
                stats,
            }
        }
        Err(error) if error.should_remove() => EnrichedExecution {
            label,
            outcome: None,
            fetch_error: Some(error),
            removal: identity.strip_prefix("article:").and_then(|id| {
                expected_article_sha256.map(|expected_article_sha256| RemovalCandidate {
                    id: id.to_string(),
                    url,
                    expected_article_sha256,
                    expected_note_sha256,
                    reason: error,
                })
            }),
            stats: LinkCaptureStats::default(),
        },
        Err(error) => {
            let outcome = if expected_article_sha256.is_some() {
                None
            } else {
                Some(
                    import_link_with_options(
                        library,
                        &url,
                        ImportOptions {
                            title,
                            saved_at,
                            state,
                        },
                    )
                    .map_err(|_| ()),
                )
            };
            EnrichedExecution {
                label,
                outcome,
                fetch_error: Some(error),
                removal: None,
                stats: LinkCaptureStats::default(),
            }
        }
    }
}

fn execute(
    library: &LibraryRoot,
    item: PlannedItem,
) -> std::result::Result<(String, SaveOutcome), String> {
    let label = item.label;
    let options = ImportOptions {
        title: item.title,
        saved_at: item.saved_at,
        state: item.state,
    };
    let result = match item.action {
        PlannedAction::Link { url } => import_link_with_options(library, &url, options),
        PlannedAction::Quote {
            origin: None,
            content,
        } => import_text_with_options(library, &content, options),
        PlannedAction::Quote {
            origin: Some(origin),
            content,
        } => execute_origin_quote(library, &origin, &content, options),
        PlannedAction::Image {
            file,
            origin: Some(origin),
            content_type,
            ..
        } => match read_nonempty_file(&file) {
            Ok(bytes) => import_image_from_origin_with_options(
                library,
                bytes,
                content_type,
                &origin,
                options,
            ),
            Err(()) => return Err(label),
        },
        PlannedAction::Image {
            file,
            origin: None,
            content_type,
            ..
        } => match read_nonempty_file(&file) {
            Ok(bytes) => import_image_with_options(library, bytes, content_type, options),
            Err(()) => return Err(label),
        },
        PlannedAction::Video {
            file,
            origin: Some(origin),
            content_type,
            ..
        } => import_video_file_from_origin_with_options(
            library,
            &file,
            content_type,
            &origin,
            options,
        ),
        PlannedAction::Video {
            file,
            origin: None,
            content_type,
            ..
        } => import_video_file_with_options(library, &file, content_type, options),
    };
    match result {
        Ok(outcome) => Ok((label, outcome)),
        Err(_) => Err(label),
    }
}

fn execute_origin_quote(
    library: &LibraryRoot,
    origin: &str,
    content: &str,
    options: ImportOptions,
) -> std::result::Result<SaveOutcome, oia_core::SaveError> {
    let parsed = Url::parse(origin).map_err(|_| {
        oia_core::SaveError::InvalidRequest("invalid imported quote origin".to_string())
    })?;
    let markdown = quote_markdown(content);
    let identity_text = normalized_identity_text(content);
    import_reading(
        library,
        SaveInput {
            link_category: None,
            product: None,
            quote_identity_markdown: None,
            kind: ReadingKind::Quote,
            lightweight: false,
            url: origin.to_string(),
            media_url: None,
            canonical_url: origin.to_string(),
            title: options
                .title
                .clone()
                .unwrap_or_else(|| "Saved quote".to_string()),
            author: None,
            site: parsed.host_str().map(str::to_string),
            saved_at: options.saved_at.clone().unwrap_or_default(),
            markdown,
            images: vec![],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            excerpt: Some(truncate_chars(&identity_text, QUOTE_EXCERPT_CHARACTERS)),
            word_count: Some(identity_text.split_whitespace().count() as u32),
            lang: None,
        },
        options.state,
    )
}

fn record_outcome(
    report: &mut RunReport,
    label: &str,
    action: &str,
    outcome: SaveOutcome,
    verbose: bool,
) {
    let (verb, counter) = match outcome.disposition {
        SaveDisposition::Saved => ("SAVED", &mut report.saved),
        SaveDisposition::Upgraded => ("UPGRADE", &mut report.upgraded),
        SaveDisposition::Duplicate => ("PRESENT", &mut report.duplicates),
    };
    *counter += 1;
    if verbose {
        report
            .verbose_lines
            .push(format!("{verb:<7} {label}: {action} ({})", outcome.id));
    }
}

fn read_nonempty_file(path: &Path) -> std::result::Result<Vec<u8>, ()> {
    match fs::read(path) {
        Ok(bytes) if !bytes.is_empty() => Ok(bytes),
        _ => Err(()),
    }
}

fn hash_nonempty_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut byte_count = 0_u64;
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        byte_count += read as u64;
    }
    if byte_count == 0 {
        return None;
    }

    let mut encoded = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Some(encoded)
}

fn http_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let parsed = Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    normalize_url(raw).ok()
}

fn normalize_tags(raw: &str, diagnostics: &mut Diagnostics) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    for source in raw.split(',') {
        let source = source.trim().trim_start_matches('#').trim();
        if source.is_empty() {
            continue;
        }
        let lowercase = source.to_lowercase();
        let mut tag = String::new();
        let mut pending_dash = false;
        for character in lowercase.chars() {
            if character.is_whitespace() {
                pending_dash = !tag.is_empty();
            } else {
                if pending_dash && !tag.ends_with('-') {
                    tag.push('-');
                }
                pending_dash = false;
                tag.push(character);
            }
        }
        let tag = tag.trim_matches('-').to_string();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > MAX_TAG_LEN {
            diagnostics.warn(WARN_LONG_TAG);
            continue;
        }
        if seen.insert(tag.clone()) {
            tags.push(tag);
        }
    }
    tags
}

fn normalize_note(raw: &str) -> Option<String> {
    (!raw.trim().is_empty()).then(|| raw.to_string())
}

fn normalized_content(raw: &str) -> Option<String> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    nonempty(&normalized).map(str::to_string)
}

fn normalized_identity_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn quote_markdown(text: &str) -> String {
    text.trim()
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(text: &str, max: usize) -> String {
    let mut characters = text.chars();
    let truncated = characters.by_ref().take(max).collect::<String>();
    if characters.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn normalize_created(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.len() < 20 || !raw.is_ascii() {
        return None;
    }
    let prefix = raw.get(..19)?;
    let bytes = prefix.as_bytes();
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if separators
        .iter()
        .any(|(index, expected)| bytes.get(*index) != Some(expected))
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7 | 10 | 13 | 16) && !byte.is_ascii_digit())
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| prefix.get(range)?.parse::<u32>().ok();
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let suffix = &raw[19..];
    let millis = if suffix == "Z" || suffix == "+00:00" {
        "000".to_string()
    } else {
        let fraction_with_zone = suffix.strip_prefix('.')?;
        let fraction = fraction_with_zone
            .strip_suffix('Z')
            .or_else(|| fraction_with_zone.strip_suffix("+00:00"))?;
        if fraction.is_empty() || !fraction.chars().all(|character| character.is_ascii_digit()) {
            return None;
        }
        let mut millis = fraction.chars().take(3).collect::<String>();
        while millis.len() < 3 {
            millis.push('0');
        }
        millis
    };
    Some(format!("{prefix}.{millis}Z"))
}

fn earliest_date(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (Some(first), None) => Some(first),
        (None, Some(second)) => Some(second),
        (None, None) => None,
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn opaque_row_label(id: &str, ordinal: usize) -> String {
    let id = id.trim();
    let safe = !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if safe {
        format!("card {id}")
    } else {
        format!("row {ordinal}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oia_core::{get_note, import_link, scan_library, set_note, write_reading, ImageBytes};
    use tempfile::TempDir;

    struct FixtureLinkMetadataFetcher;

    impl LinkMetadataFetcher for FixtureLinkMetadataFetcher {
        fn fetch(&self, url: &str) -> std::result::Result<LinkMetadataCapture, LinkFetchError> {
            if url.contains("gone.example") {
                return Err(LinkFetchError::Gone);
            }
            if url.contains("unreachable.example") {
                return Err(LinkFetchError::Unreachable);
            }
            Ok(fixture_link_capture(url))
        }
    }

    struct MutatingLinkMetadataFetcher {
        library: PathBuf,
    }

    impl LinkMetadataFetcher for MutatingLinkMetadataFetcher {
        fn fetch(&self, url: &str) -> std::result::Result<LinkMetadataCapture, LinkFetchError> {
            let library = LibraryRoot::new(&self.library).unwrap();
            let id = url_id(url).unwrap();
            let article_path = library.article_path(&id);
            let mut reading = parse_reading(&fs::read_to_string(&article_path).unwrap()).unwrap();
            reading.metadata.title = "Concurrent local title".to_string();
            write_reading(
                &library,
                reading.metadata,
                "Concurrent lightweight body".to_string(),
            )
            .unwrap();

            if url.contains("gone.example") {
                Err(LinkFetchError::Gone)
            } else {
                Ok(fixture_link_capture(url))
            }
        }
    }

    struct AddingNoteLinkMetadataFetcher {
        library: PathBuf,
    }

    impl LinkMetadataFetcher for AddingNoteLinkMetadataFetcher {
        fn fetch(&self, url: &str) -> std::result::Result<LinkMetadataCapture, LinkFetchError> {
            let library = LibraryRoot::new(&self.library).unwrap();
            set_note(
                &library,
                &url_id(url).unwrap(),
                "Concurrent note added during metadata fetch",
            )
            .unwrap();
            Err(LinkFetchError::Gone)
        }
    }

    fn fixture_link_capture(url: &str) -> LinkMetadataCapture {
        let preview_url = "https://assets.example/social.png".to_string();
        let favicon_url = "https://assets.example/favicon.ico".to_string();
        LinkMetadataCapture {
            link_category: None,
            product: url.contains("product").then(|| oia_core::ProductMetadata {
                version: 1,
                price: Some("229.00".into()),
                currency: Some("EUR".into()),
                price_is_from: false,
                captured_at: Some("2026-10-10T10:00:00Z".into()),
            }),
            canonical_url: url.to_string(),
            title: Some("Social title".to_string()),
            site: Some("Fixture Journal".to_string()),
            author: None,
            lang: Some("en".to_string()),
            excerpt: Some("Social excerpt".to_string()),
            theme_color: Some("rgb(18 52 86)".to_string()),
            images: vec![
                ImageBytes {
                    url: preview_url.clone(),
                    content_type: "image/png".to_string(),
                    bytes: b"social-png".to_vec(),
                },
                ImageBytes {
                    url: favicon_url.clone(),
                    content_type: "image/x-icon".to_string(),
                    bytes: b"favicon-ico".to_vec(),
                },
            ],
            preview_url: Some(preview_url),
            favicon_url: Some(favicon_url),
        }
    }

    fn setup() -> (TempDir, TempDir) {
        (TempDir::new().unwrap(), TempDir::new().unwrap())
    }

    fn options(export: &Path, library: &Path, write: bool) -> RunOptions {
        RunOptions {
            export: export.to_path_buf(),
            library: library.to_path_buf(),
            write,
            verbose: false,
            enrich_links: false,
            workers: 2,
        }
    }

    fn fixture_csv(rows: &str) -> String {
        format!("id,type,title,url,content,note,tags,created\n{rows}")
    }

    #[test]
    fn nested_export_bom_and_embedded_newline_are_supported() {
        let (outer, library) = setup();
        let nested = outer.path().join("mymind");
        fs::create_dir(&nested).unwrap();
        fs::write(
            nested.join(CSV_FILENAME),
            "\u{feff}id,type,title,url,content,note,tags,created\n\
             c1,Content,Thought,https://example.com/source,\"first line\nsecond line\",,Ideas,2024-03-01T12:00:00Z\n",
        )
        .unwrap();

        let report = run(options(outer.path(), library.path(), true)).unwrap();
        assert_eq!(report.saved, 1);
        assert_eq!(report.errors, 0);
        let reading = scan_library(&LibraryRoot::new(library.path()).unwrap())
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(reading.metadata.kind, ReadingKind::Quote);
        assert_eq!(reading.metadata.url, "https://example.com/source");
        assert!(reading.body.contains("first line\n> second line"));
    }

    #[test]
    fn web_links_capture_social_preview_and_favicon_assets() {
        let (export, library) = setup();
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv("web,WebPage,,https://good.example/article,,,,2024-03-01T12:00:00Z\n"),
        )
        .unwrap();

        let mut run_options = options(export.path(), library.path(), true);
        run_options.enrich_links = true;
        let report =
            run_with_fetcher(run_options, Some(Arc::new(FixtureLinkMetadataFetcher))).unwrap();
        assert_eq!(report.saved, 1);
        let root = LibraryRoot::new(library.path()).unwrap();
        let reading = scan_library(&root).unwrap().pop().unwrap();
        let preview = reading
            .metadata
            .preview_asset
            .expect("the social image should be stored as a local preview");
        let favicon = reading
            .metadata
            .favicon_asset
            .expect("the favicon should be stored as a separate local asset");
        assert!(root.reading_dir(&reading.id).join(preview).is_file());
        assert!(root.reading_dir(&reading.id).join(favicon).is_file());
        assert_eq!(reading.metadata.theme_color.as_deref(), Some("#123456"));
    }

    #[test]
    fn imported_product_links_keep_price_metadata_and_original_identity() {
        let (export, library) = setup();
        let url = "https://good.example/product";
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv(&format!("product,WebPage,,{url},,,,2024-03-01T12:00:00Z\n")),
        )
        .unwrap();
        let mut run_options = options(export.path(), library.path(), true);
        run_options.enrich_links = true;
        let report =
            run_with_fetcher(run_options, Some(Arc::new(FixtureLinkMetadataFetcher))).unwrap();
        assert_eq!(report.saved, 1);
        let reading = scan_library(&LibraryRoot::new(library.path()).unwrap())
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(reading.metadata.id, url_id(url).unwrap());
        assert!(reading.metadata.lightweight);
        assert_eq!(
            reading.metadata.product.unwrap().price.as_deref(),
            Some("229")
        );
    }

    #[test]
    fn unavailable_web_links_in_a_fresh_export_are_counted_as_skipped() {
        let (export, library) = setup();
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv(
                "gone,WebPage,,https://gone.example/article,,,,2024-03-01T12:00:00Z\n\
                 unreachable,WebPage,,https://unreachable.example/article,,,,2024-03-01T12:00:00Z\n",
            ),
        )
        .unwrap();

        let mut run_options = options(export.path(), library.path(), true);
        run_options.enrich_links = true;
        let report =
            run_with_fetcher(run_options, Some(Arc::new(FixtureLinkMetadataFetcher))).unwrap();

        assert_eq!(report.planned, 2);
        assert_eq!(report.saved, 0);
        assert_eq!(report.upgraded, 0);
        assert_eq!(report.removed, 0);
        assert_eq!(report.duplicates, 0);
        assert_eq!(report.skipped, 2);
        assert_eq!(report.errors, 0);
        assert_eq!(
            report.saved + report.upgraded + report.duplicates + report.skipped,
            report.planned
        );
        assert!(scan_library(&LibraryRoot::new(library.path()).unwrap())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn existing_migration_enriches_live_links_and_removes_dead_ones() {
        let library = TempDir::new().unwrap();
        let root = LibraryRoot::new(library.path()).unwrap();
        for url in [
            "https://good.example/article",
            "https://gone.example/article",
            "https://unreachable.example/article",
        ] {
            import_link(&root, url).unwrap();
        }

        let report = enrich_existing_links_with_fetcher(
            EnrichExistingOptions {
                library: library.path().to_path_buf(),
                write: true,
                verbose: false,
                workers: 3,
                expected_count: Some(3),
                expected_digest: None,
            },
            Some(Arc::new(FixtureLinkMetadataFetcher)),
        )
        .unwrap();

        assert_eq!(report.upgraded, 1);
        assert_eq!(report.removed, 2);
        let readings = scan_library(&root).unwrap();
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].metadata.url, "https://good.example/article");
        assert_eq!(readings[0].metadata.theme_color.as_deref(), Some("#123456"));
    }

    #[test]
    fn existing_migration_skips_enrichment_and_deletion_after_concurrent_edits() {
        let library = TempDir::new().unwrap();
        let root = LibraryRoot::new(library.path()).unwrap();
        for url in [
            "https://good.example/concurrent",
            "https://gone.example/concurrent",
        ] {
            import_link(&root, url).unwrap();
        }

        let report = enrich_existing_links_with_fetcher(
            EnrichExistingOptions {
                library: library.path().to_path_buf(),
                write: true,
                verbose: false,
                workers: 1,
                expected_count: Some(2),
                expected_digest: None,
            },
            Some(Arc::new(MutatingLinkMetadataFetcher {
                library: library.path().to_path_buf(),
            })),
        )
        .unwrap();

        assert_eq!(report.planned, 2);
        assert_eq!(report.upgraded, 0);
        assert_eq!(report.removed, 0);
        assert_eq!(report.unchanged, 2);
        assert_eq!(report.errors, 0);
        let readings = scan_library(&root).unwrap();
        assert_eq!(readings.len(), 2);
        assert!(readings
            .iter()
            .all(|reading| reading.metadata.title == "Concurrent local title"));
        assert!(readings
            .iter()
            .all(|reading| reading.metadata.preview_asset.is_none()));
    }

    #[test]
    fn existing_migration_preserves_a_note_added_during_dead_link_fetch() {
        let library = TempDir::new().unwrap();
        let root = LibraryRoot::new(library.path()).unwrap();
        let url = "https://gone.example/concurrent-note";
        let id = url_id(url).unwrap();
        import_link(&root, url).unwrap();

        let report = enrich_existing_links_with_fetcher(
            EnrichExistingOptions {
                library: library.path().to_path_buf(),
                write: true,
                verbose: false,
                workers: 1,
                expected_count: Some(1),
                expected_digest: None,
            },
            Some(Arc::new(AddingNoteLinkMetadataFetcher {
                library: library.path().to_path_buf(),
            })),
        )
        .unwrap();

        assert_eq!(report.planned, 1);
        assert_eq!(report.removed, 0);
        assert_eq!(report.unchanged, 1);
        assert_eq!(report.errors, 0);
        assert!(root.article_path(&id).is_file());
        assert_eq!(
            get_note(&root, &id).unwrap().as_deref(),
            Some("Concurrent note added during metadata fetch")
        );
    }

    #[test]
    fn systemic_unreachable_guard_counts_retained_links_as_unchanged() {
        let library = TempDir::new().unwrap();
        let root = LibraryRoot::new(library.path()).unwrap();
        for ordinal in 0..3 {
            import_link(
                &root,
                &format!("https://unreachable.example/article-{ordinal}"),
            )
            .unwrap();
        }

        let report = enrich_existing_links_with_fetcher(
            EnrichExistingOptions {
                library: library.path().to_path_buf(),
                write: true,
                verbose: false,
                workers: 3,
                expected_count: Some(3),
                expected_digest: None,
            },
            Some(Arc::new(FixtureLinkMetadataFetcher)),
        )
        .unwrap();

        assert_eq!(report.planned, 3);
        assert_eq!(report.upgraded, 0);
        assert_eq!(report.removed, 0);
        assert_eq!(report.unchanged, 3);
        assert_eq!(report.errors, 0);
        assert_eq!(scan_library(&root).unwrap().len(), 3);
        assert_eq!(
            report.upgraded + report.removed + report.unchanged,
            report.planned
        );
    }

    #[test]
    fn svg_is_imported_by_exact_row_id() {
        let (export, library) = setup();
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv("svg1,Image,,,,,,2024-03-01T12:00:00Z\n"),
        )
        .unwrap();
        fs::write(export.path().join("svg1.svg"), b"<svg></svg>").unwrap();

        let report = run(options(export.path(), library.path(), true)).unwrap();
        assert_eq!(report.saved, 1);
        let reading = scan_library(&LibraryRoot::new(library.path()).unwrap())
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(reading.metadata.kind, ReadingKind::Image);
        assert_eq!(reading.metadata.title, "Imported image");
        assert!(reading.metadata.preview_asset.unwrap().ends_with(".svg"));
    }

    #[test]
    fn video_magic_is_sniffed_for_blob_mov_and_mkv_data() {
        let dir = TempDir::new().unwrap();
        let quicktime = dir.path().join("one.blob");
        fs::write(&quicktime, b"\0\0\0\x18ftypqt  \0\0\0\0").unwrap();
        assert_eq!(
            classify_file(&quicktime),
            (ExportedFileKind::Video, Some("video/quicktime"))
        );

        let m4v = dir.path().join("two.mov");
        fs::write(&m4v, b"\0\0\0\x18ftypM4V \0\0\0\0").unwrap();
        assert_eq!(
            classify_file(&m4v),
            (ExportedFileKind::Video, Some("video/m4v"))
        );

        let webm = dir.path().join("three.blob");
        fs::write(&webm, b"\x1a\x45\xdf\xa3\x42\x82\x84webm").unwrap();
        assert_eq!(
            classify_file(&webm),
            (ExportedFileKind::Video, Some("video/webm"))
        );
    }

    #[test]
    fn unsupported_rows_are_structured_skips_not_errors() {
        let (export, library) = setup();
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv(
                "d,Document,,,,,,2024-03-01T12:00:00Z\n\
                 dl,Document,,https://example.com/document,,,,2024-03-01T12:00:00Z\n\
                 s,Screenshot,No source,,,,,2024-03-01T12:00:00Z\n\
                 q,Quotation,,,,,,2024-03-01T12:00:00Z\n\
                 ql,Quotation,,https://example.com/source,,,,2024-03-01T12:00:00Z\n\
                 p,Placeholder,,,,,,2024-03-01T12:00:00Z\n",
            ),
        )
        .unwrap();
        fs::write(export.path().join("d.pdf"), b"%PDF").unwrap();
        fs::write(export.path().join("dl.pdf"), b"%PDF").unwrap();

        let report = run(options(export.path(), library.path(), false)).unwrap();
        assert_eq!(report.planned, 2);
        assert_eq!(report.skipped, 5);
        assert_eq!(report.errors, 0);
    }

    #[test]
    fn malformed_csv_record_blocks_all_writes() {
        let (export, library) = setup();
        fs::write(
            export.path().join(CSV_FILENAME),
            "id,type,title,url,content,note,tags,created\n\
             bad,WebPage,Title,https://example.com,,,tag\n",
        )
        .unwrap();

        let report = run(options(export.path(), library.path(), true)).unwrap();
        assert_eq!(report.errors, 1);
        assert!(report.render().contains("blocking errors"));
        assert!(!library.path().join("articles").exists());
    }

    #[test]
    fn nested_export_and_library_are_rejected() {
        let export = TempDir::new().unwrap();
        fs::write(export.path().join(CSV_FILENAME), fixture_csv("")).unwrap();
        let library = export.path().join("library");
        fs::create_dir(&library).unwrap();

        let error = run(options(export.path(), &library, false)).unwrap_err();
        assert!(error.to_string().contains("must not contain one another"));
    }

    #[test]
    fn duplicate_rows_coalesce_tags_date_title_and_identical_note() {
        let (export, library) = setup();
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv(
                "a,WebPage,First,https://www.example.com/post?utm_source=one,,same note,One,2024-03-03T12:00:00Z\n\
                 b,WebPage,Latest,https://example.com/post,,same note,Two,2024-03-01T12:00:00Z\n",
            ),
        )
        .unwrap();

        let report = run(options(export.path(), library.path(), true)).unwrap();
        assert_eq!(report.source_rows, 2);
        assert_eq!(report.planned, 1);
        assert_eq!(report.merged_duplicates, 1);
        assert_eq!(report.saved, 1);
        let root = LibraryRoot::new(library.path()).unwrap();
        let reading = scan_library(&root).unwrap().pop().unwrap();
        assert_eq!(reading.metadata.title, "Latest");
        assert_eq!(reading.metadata.saved_at, "2024-03-01T12:00:00.000Z");
        assert_eq!(reading.metadata.tags, vec!["one", "two"]);
        assert_eq!(
            get_note(&root, &reading.id).unwrap().as_deref(),
            Some("same note")
        );
    }

    #[test]
    fn conflicting_duplicate_notes_block_writes() {
        let (export, library) = setup();
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv(
                "a,WebPage,First,https://example.com/post,,one,,2024-03-01T12:00:00Z\n\
                 b,WebPage,Second,https://example.com/post,,two,,2024-03-02T12:00:00Z\n",
            ),
        )
        .unwrap();

        let report = run(options(export.path(), library.path(), true)).unwrap();
        assert_eq!(report.errors, 1);
        assert!(report.render().contains("blocking errors"));
        assert!(!library.path().join("articles").exists());
    }

    #[test]
    fn origin_media_coalesces_same_origin_and_bytes_only() {
        let (export, library) = setup();
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv(
                "i1,Image,First,https://example.com/gallery?utm_source=one,,,,2024-03-01T12:00:00Z\n\
                 i2,Image,Latest,https://www.example.com/gallery,,,,2024-03-02T12:00:00Z\n\
                 i3,Image,Elsewhere,https://other.example/gallery,,,,2024-03-03T12:00:00Z\n",
            ),
        )
        .unwrap();
        for id in ["i1", "i2", "i3"] {
            fs::write(export.path().join(format!("{id}.png")), b"same pixels").unwrap();
        }

        let report = run(options(export.path(), library.path(), true)).unwrap();
        assert_eq!(report.planned, 2);
        assert_eq!(report.merged_duplicates, 1);
        assert_eq!(report.saved, 2);
        let readings = scan_library(&LibraryRoot::new(library.path()).unwrap()).unwrap();
        assert_eq!(readings.len(), 2);
        assert!(readings
            .iter()
            .any(|reading| reading.metadata.url == "https://example.com/gallery"));
        assert!(readings
            .iter()
            .any(|reading| reading.metadata.url == "https://other.example/gallery"));
    }

    #[test]
    fn default_output_is_aggregate_and_does_not_echo_private_values() {
        let (export, library) = setup();
        let private_url = "https://private.example/a-secret";
        let private_tag = "a-very-private-tag-that-is-too-long";
        fs::write(
            export.path().join(CSV_FILENAME),
            fixture_csv(&format!(
                "safe-id,WebPage,Private title,{private_url},,,{private_tag},2024-03-01T12:00:00Z\n"
            )),
        )
        .unwrap();

        let rendered = run(options(export.path(), library.path(), false))
            .unwrap()
            .render();
        assert!(!rendered.contains(private_url));
        assert!(!rendered.contains(private_tag));
        assert!(!rendered.contains(export.path().to_string_lossy().as_ref()));
        assert!(!rendered.contains("safe-id"));
        assert!(rendered.contains("overlong tags omitted"));
    }
}
