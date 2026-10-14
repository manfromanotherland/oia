// SPDX-License-Identifier: MIT

//! An at-least-once file handoff into the canonical reading importer.
//!
//! External writers publish ordinary files or sealed `.cuttingscapture.zip`
//! archives. We work from a private, bounded snapshot, keep failures in place,
//! and only remove the unchanged input after verifying the saved reading.
//! URL imports share the bounded source/category resolver. Version-2 Instagram
//! requests require an explicit downloader supplied by the native app; no index
//! writes occur here.

use std::{
    collections::HashSet,
    fs::{self, File, Metadata},
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, SystemTime},
};

use anyhow::{bail, ensure, Context, Result};
use rustix::fs::{open, openat, renameat_with, unlinkat, AtFlags, Mode, OFlags, RenameFlags};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

use crate::{
    begin_browser_video_import, import_image_with_options, import_reading,
    import_text_with_options, import_video_file_with_options, parse_reading, save_selection,
    save_url, selection_kind, BrowserVideoImportInput, ImportOptions, ImportedReadingState,
    LibraryRoot, ReadingKind, SaveDisposition, SaveInput, SaveOutcome, SelectionKind,
    SelectionSaveInput, UrlSaveRequest, MAX_BROWSER_VIDEO_BYTES,
};

const QUIET_PERIOD: Duration = Duration::from_secs(2);
const MAX_SMALL_BYTES: u64 = 40 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_CAPTURE_BYTES: u64 = MAX_BROWSER_VIDEO_BYTES + 2 * MAX_MANIFEST_BYTES;
const MAX_ARCHIVE_ENTRIES: usize = 128;

type UrlSaver<'a> = dyn Fn(&LibraryRoot, UrlSaveRequest) -> Result<SaveOutcome> + 'a;
struct FetchedArticle {
    html: String,
    url: String,
}

type ArticleFetcher<'a> = dyn Fn(&str) -> Result<FetchedArticle> + 'a;

fn fetch_article_html(url: &str) -> Result<FetchedArticle> {
    let (bytes, content_type, final_url) = crate::product::fetch_public_bytes(
        url,
        crate::article_html::MAX_ARTICLE_HTML_BYTES,
        Duration::from_secs(8),
    )?;
    ensure!(
        content_type.as_deref().is_none_or(|value| {
            matches!(
                value.split(';').next().unwrap_or("").trim(),
                "text/html" | "application/xhtml+xml"
            )
        }),
        "The shared URL did not return an HTML page."
    );
    Ok(FetchedArticle {
        html: String::from_utf8(bytes).context("The shared article must use UTF-8 HTML.")?,
        url: final_url,
    })
}

fn save_inbox_url(library: &LibraryRoot, request: UrlSaveRequest) -> Result<SaveOutcome> {
    Ok(save_url(library, request)?)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InboxReport {
    pub saved: u32,
    pub duplicates: u32,
    pub pending: u32,
    pub issues: Vec<InboxIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxIssue {
    pub name: String,
    pub message: String,
}

/// Process currently available direct children of `inbox/`.
///
/// A platform adapter may defer exact basenames whose cloud copies are not
/// current. Missing/changed/unreadable sources remain pending. Stable invalid
/// sources remain untouched and are reported for the user to inspect. A crash
/// after saving but before removing input is safe: the next pass deduplicates.
pub fn process_inbox(library: &LibraryRoot, deferred_names: &[String]) -> Result<InboxReport> {
    process_inbox_at(library, deferred_names, SystemTime::now())
}

fn process_inbox_at(
    library: &LibraryRoot,
    deferred_names: &[String],
    now: SystemTime,
) -> Result<InboxReport> {
    process_inbox_with_resolver_at(library, deferred_names, now, None)
}

pub fn process_inbox_with_instagram(
    library: &LibraryRoot,
    deferred_names: &[String],
    python: &Path,
    script: &Path,
) -> Result<InboxReport> {
    let resolver = |request: &crate::instagram::InstagramRequest| {
        crate::instagram::download(python, script, request)
    };
    process_inbox_with_resolver_at(library, deferred_names, SystemTime::now(), Some(&resolver))
}

fn process_inbox_with_resolver_at(
    library: &LibraryRoot,
    deferred_names: &[String],
    now: SystemTime,
    resolver: Option<&crate::instagram::Resolver<'_>>,
) -> Result<InboxReport> {
    process_inbox_with_savers_at(library, deferred_names, now, resolver, &save_inbox_url)
}

fn process_inbox_with_savers_at(
    library: &LibraryRoot,
    deferred_names: &[String],
    now: SystemTime,
    resolver: Option<&crate::instagram::Resolver<'_>>,
    url_saver: &UrlSaver<'_>,
) -> Result<InboxReport> {
    process_inbox_with_fetchers_at(
        library,
        deferred_names,
        now,
        resolver,
        url_saver,
        &fetch_article_html,
    )
}

fn process_inbox_with_fetchers_at(
    library: &LibraryRoot,
    deferred_names: &[String],
    now: SystemTime,
    resolver: Option<&crate::instagram::Resolver<'_>>,
    url_saver: &UrlSaver<'_>,
    article_fetcher: &ArticleFetcher<'_>,
) -> Result<InboxReport> {
    let inbox = library.inbox_dir();
    match fs::symlink_metadata(&inbox) {
        Ok(metadata) => ensure!(
            metadata.file_type().is_dir(),
            "Inbox must be a real directory"
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&inbox)?,
        Err(error) => return Err(error.into()),
    }
    let directory = open_regular_directory(&inbox)?;
    let directory_metadata = directory.metadata()?;
    let mut entries = fs::read_dir(&inbox)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut report = InboxReport::default();
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // Never follow arbitrary folders, symlinks, or provider staging files.
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => continue,
            Err(_) => {
                report.pending += 1;
                continue;
            }
        };
        if deferred_names.iter().any(|deferred| deferred == name)
            || is_dataless(&metadata)
            || metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_none_or(|age| age < QUIET_PERIOD)
        {
            report.pending += 1;
            continue;
        }
        if metadata.len() > MAX_CAPTURE_BYTES {
            report
                .issues
                .push(issue(name, "The file exceeds the 1 GiB Inbox limit."));
            continue;
        }
        let mut snapshot = match Snapshot::read(&path, &metadata) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) | Err(_) => {
                report.pending += 1;
                continue;
            }
        };
        // Pinning the source descriptor cannot pin its parent pathname. Detect
        // a replaced Inbox before writes and again before removing any input.
        if !directory_is_unchanged(&inbox, &directory_metadata) {
            report.pending += 1;
            continue;
        }
        let mut outcomes = Vec::new();
        let result = if name.to_ascii_lowercase().ends_with(".cuttingscapture.zip") {
            import_capture(
                library,
                &mut snapshot.file,
                &mut outcomes,
                resolver,
                url_saver,
                article_fetcher,
            )
        } else {
            import_payload(
                library,
                snapshot.file.path(),
                name,
                &Origin::default(),
                None,
                url_saver,
            )
            .map(|outcome| outcomes.push(outcome))
        };
        // A multi-item capture can encounter a later storage failure after
        // earlier readings were committed. Report those writes immediately,
        // retain the whole capture, and deduplicate them on the next retry.
        for outcome in outcomes {
            match outcome.disposition {
                SaveDisposition::Duplicate => report.duplicates += 1,
                SaveDisposition::Saved | SaveDisposition::Upgraded => report.saved += 1,
            }
        }
        match result {
            Ok(()) => {
                let removed = directory_is_unchanged(&inbox, &directory_metadata)
                    && snapshot
                        .remove_if_unchanged(&directory, name)
                        .unwrap_or(false);
                if !removed {
                    report.pending += 1;
                }
            }
            Err(error) => report.issues.push(issue(name, &format!("{error:#}"))),
        }
    }
    Ok(report)
}

fn issue(name: &str, message: &str) -> InboxIssue {
    InboxIssue {
        name: name.to_string(),
        message: message.to_string(),
    }
}

fn open_regular_directory(path: &Path) -> io::Result<File> {
    Ok(open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

fn open_regular_file(path: &Path) -> io::Result<File> {
    let file: File = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )?
    .into();
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(file)
}

fn open_regular_file_at(directory: &File, name: &str) -> io::Result<File> {
    let file: File = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )?
    .into();
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(file)
}

fn directory_is_unchanged(path: &Path, before: &Metadata) -> bool {
    fs::symlink_metadata(path).is_ok_and(|after| {
        after.file_type().is_dir() && before.dev() == after.dev() && before.ino() == after.ino()
    })
}

fn same_file(before: &Metadata, after: &Metadata) -> bool {
    same_file_contents(before, after)
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

fn same_file_contents(before: &Metadata, after: &Metadata) -> bool {
    after.file_type().is_file()
        && before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
}

#[cfg(target_os = "macos")]
fn is_dataless(metadata: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt as _;
    metadata.st_flags() & 0x4000_0000 != 0 // UF_DATALESS: ask File Provider to hydrate first.
}

#[cfg(not(target_os = "macos"))]
fn is_dataless(_: &Metadata) -> bool {
    false
}

struct Snapshot {
    file: NamedTempFile,
    metadata: Metadata,
    sha256: String,
}

enum SourceClaim {
    Missing,
    Changed,
    Claimed(String),
}

fn recovery_name(name: &str) -> String {
    let prefix = format!("recovered-{}-", crate::new_id());
    // Preserve the complete transport suffix and stay within NAME_MAX even
    // when the original filename is already at the filesystem's byte limit.
    let suffix_start = if name.to_ascii_lowercase().ends_with(".cuttingscapture.zip") {
        name.len() - ".cuttingscapture.zip".len()
    } else {
        name.rfind('.').unwrap_or(name.len())
    };
    let (stem, suffix) = name.split_at(suffix_start);
    let mut length = stem
        .len()
        .min(255_usize.saturating_sub(prefix.len() + suffix.len()));
    while !stem.is_char_boundary(length) {
        length -= 1;
    }
    format!("{prefix}{}{suffix}", &stem[..length])
}

impl Snapshot {
    fn read(path: &Path, expected: &Metadata) -> Result<Option<Self>> {
        let mut source = open_regular_file(path)?;
        if !same_file(expected, &source.metadata()?) {
            return Ok(None);
        }
        let mut file = NamedTempFile::new()?;
        let (size, sha256) = copy_hashed(&mut source, &mut file, MAX_CAPTURE_BYTES)?;
        if size != expected.len() || !same_file(expected, &source.metadata()?) {
            return Ok(None);
        }
        file.flush()?;
        Ok(Some(Self {
            file,
            metadata: expected.clone(),
            sha256,
        }))
    }

    fn remove_if_unchanged(&self, directory: &File, name: &str) -> Result<bool> {
        match self.claim_source(directory, name, || {})? {
            SourceClaim::Missing => Ok(true),
            SourceClaim::Changed => Ok(false),
            SourceClaim::Claimed(name) => self.remove_claimed_if_unchanged(directory, &name),
        }
    }

    /// The callback is an internal seam for deterministic replacement tests.
    /// Production callers perform no work between the preflight and claim.
    fn claim_source(
        &self,
        directory: &File,
        name: &str,
        before_claim: impl FnOnce(),
    ) -> Result<SourceClaim> {
        let source = match open_regular_file_at(directory, name) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(SourceClaim::Missing)
            }
            Err(_) => return Ok(SourceClaim::Changed),
        };
        if !same_file(&self.metadata, &source.metadata()?) {
            return Ok(SourceClaim::Changed);
        }
        before_claim();
        let recovery = recovery_name(name);
        match renameat_with(
            directory,
            name,
            directory,
            &recovery,
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => {}
            Err(rustix::io::Errno::NOENT) => return Ok(SourceClaim::Missing),
            Err(error) => return Err(error.into()),
        }
        // A crash from this point leaves a visible, normally importable input.
        // Never restore with an overwriting rename: a new arrival can already
        // own the original name. All further operations use the pinned parent.
        directory.sync_all()?;
        Ok(SourceClaim::Claimed(recovery))
    }

    fn remove_claimed_if_unchanged(&self, directory: &File, name: &str) -> Result<bool> {
        let mut source = match open_regular_file_at(directory, name) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(_) => return Ok(false),
        };
        let claimed_metadata = source.metadata()?;
        // Rename changes ctime. Match the original identity/size/mtime, then
        // require the new ctime to stay fixed throughout checksum verification.
        if !same_file_contents(&self.metadata, &claimed_metadata) {
            return Ok(false);
        }
        let (size, hash) = copy_hashed(&mut source, &mut io::sink(), MAX_CAPTURE_BYTES)?;
        if size != self.metadata.len()
            || hash != self.sha256
            || !same_file(&claimed_metadata, &source.metadata()?)
            || !open_regular_file_at(directory, name)
                .and_then(|file| file.metadata())
                .is_ok_and(|after| same_file(&claimed_metadata, &after))
        {
            return Ok(false);
        }
        match unlinkat(directory, name, AtFlags::empty()) {
            Ok(()) => {
                directory.sync_all()?;
                Ok(true)
            }
            Err(rustix::io::Errno::NOENT) => Ok(true),
            Err(error) => Err(error.into()),
        }
    }
}

fn copy_hashed(
    reader: &mut impl Read,
    writer: &mut impl Write,
    limit: u64,
) -> Result<(u64, String)> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut hasher = Sha256::new();
    let mut size = 0;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        ensure!(size <= limit, "The file exceeds its supported size limit.");
        hasher.update(&buffer[..count]);
        writer.write_all(&buffer[..count])?;
    }
    Ok((size, hex::encode(hasher.finalize())))
}

#[derive(Default, Deserialize)]
struct Origin {
    url: Option<String>,
    title: Option<String>,
    canonical_url: Option<String>,
    site_name: Option<String>,
}

impl Origin {
    fn url(&self) -> Result<Option<String>> {
        self.url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .map(http_url)
            .transpose()
    }
    fn canonical_url(&self, origin: &str) -> String {
        self.canonical_url
            .as_deref()
            .and_then(|url| http_url(url).ok())
            .unwrap_or_else(|| origin.to_string())
    }
    fn title(&self, fallback: &str) -> String {
        self.title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(fallback)
            .to_string()
    }
    fn site(&self, origin: &str) -> Option<String> {
        self.site_name
            .clone()
            .filter(|site| !site.trim().is_empty())
            .or_else(|| {
                url::Url::parse(origin)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_string))
            })
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    capture_id: String,
    captured_at: String,
    capture_type: Option<CaptureType>,
    #[serde(default)]
    origin: Origin,
    text: Option<String>,
    instagram_url: Option<String>,
    article: Option<ArticlePayload>,
    #[serde(default)]
    attachments: Vec<Attachment>,
}

/// Capture intent is distinct from the core's inferred book/movie/quote category.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum CaptureType {
    Url,
    Selection,
    Text,
    Media,
    Article,
}

#[derive(Deserialize)]
struct ArticlePayload {
    #[serde(flatten)]
    attachment: Attachment,
    format: ArticleFormat,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ArticleFormat {
    Html,
}

fn validate_capture_type(manifest: &Manifest, origin_url: Option<&str>) -> Result<()> {
    if manifest.version < 3 {
        ensure!(
            manifest.article.is_none(),
            "Article content requires capture version 4."
        );
        return Ok(());
    }
    ensure!(
        manifest.version == 4 || manifest.article.is_none(),
        "Article content requires capture version 4."
    );
    let capture_type = manifest
        .capture_type
        .context("Typed captures require a capture_type.")?;
    let text = manifest.text.as_deref().map(str::trim).unwrap_or("");
    ensure!(
        text.len() as u64 <= MAX_SMALL_BYTES,
        "Text must be 40 MiB or smaller."
    );
    match capture_type {
        CaptureType::Url => {
            let text_url = (!text.is_empty()).then(|| http_url(text)).transpose()?;
            ensure!(
                manifest.article.is_none()
                    && manifest.attachments.is_empty()
                    && (origin_url.is_some() || text_url.is_some())
                    && text_url
                        .as_deref()
                        .is_none_or(|url| origin_url.is_none_or(|origin| origin == url)),
                "A URL capture must contain one source URL and no other payloads."
            );
        }
        CaptureType::Selection => ensure!(
            origin_url.is_some()
                && !text.is_empty()
                && text.len() <= crate::MAX_SELECTION_TEXT_BYTES
                && (manifest.attachments.is_empty()
                    || (manifest.version == 4 && manifest.article.is_some())),
            "A selection capture requires a source URL and 1–65536 bytes of selected text."
        ),
        CaptureType::Text => ensure!(
            origin_url.is_none()
                && !text.is_empty()
                && manifest.attachments.is_empty()
                && manifest.article.is_none(),
            "A text capture requires source-less text and no attachments."
        ),
        CaptureType::Media => ensure!(
            !manifest.attachments.is_empty() && text.is_empty() && manifest.article.is_none(),
            "A media capture requires attachments and no additional text."
        ),
        CaptureType::Article => {
            let text_url = (!text.is_empty()).then(|| http_url(text)).transpose()?;
            ensure!(
                manifest.version == 4
                    && (origin_url.is_some() || text_url.is_some())
                    && text_url
                        .as_deref()
                        .is_none_or(|url| origin_url.is_none_or(|origin| origin == url))
                    && (manifest.attachments.is_empty() || manifest.article.is_some()),
                "An article capture requires one source URL and optional HTML content."
            );
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct Attachment {
    path: String,
    byte_count: Option<u64>,
    sha256: Option<String>,
    source_url: Option<String>,
    content_type: Option<String>,
}

fn safe_archive_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && !name.contains('\0')
        && !name.starts_with('/')
        && name
            .trim_end_matches('/')
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && !name.contains(':')
}

fn extract_capture_attachment<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    attachment: &Attachment,
    limit: u64,
    referenced: &mut HashSet<String>,
) -> Result<NamedTempFile> {
    ensure!(
        safe_archive_name(&attachment.path)
            && attachment.path != "manifest.json"
            && referenced.insert(attachment.path.clone()),
        "The capture attachment path is invalid."
    );
    let mut entry = archive
        .by_name(&attachment.path)
        .context("A capture attachment is missing.")?;
    ensure!(!entry.is_dir(), "A capture attachment is not a file.");
    let expected = entry.size();
    ensure!(
        expected <= limit,
        "A capture attachment exceeds its supported size limit."
    );
    let mut file = NamedTempFile::new()?;
    let (actual, hash) = copy_hashed(&mut entry, &mut file, limit)?;
    ensure!(
        actual == expected && attachment.byte_count.is_none_or(|size| size == actual),
        "A capture attachment has an unexpected size."
    );
    ensure!(
        attachment
            .sha256
            .as_ref()
            .is_none_or(|expected| expected.eq_ignore_ascii_case(&hash)),
        "A capture attachment has an unexpected checksum."
    );
    file.flush()?;
    Ok(file)
}

fn import_capture(
    library: &LibraryRoot,
    snapshot: &mut NamedTempFile,
    outcomes: &mut Vec<SaveOutcome>,
    resolver: Option<&crate::instagram::Resolver<'_>>,
    url_saver: &UrlSaver<'_>,
    article_fetcher: &ArticleFetcher<'_>,
) -> Result<()> {
    snapshot.rewind()?;
    let mut archive =
        zip::ZipArchive::new(snapshot).context("The capture archive is incomplete or invalid.")?;
    ensure!(
        archive.len() <= MAX_ARCHIVE_ENTRIES,
        "The capture contains too many files."
    );
    let mut names = HashSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        ensure!(
            safe_archive_name(entry.name()),
            "The capture contains an unsafe file path."
        );
        ensure!(
            names.insert(entry.name().to_string()),
            "The capture contains duplicate file paths."
        );
        let file_type = entry.unix_mode().map(|mode| mode & 0o170000);
        ensure!(
            file_type.is_none_or(|kind| matches!(kind, 0 | 0o100000 | 0o040000)),
            "The capture contains a symbolic link or special file."
        );
        ensure!(
            if entry.is_dir() {
                entry.size() == 0 && file_type.is_none_or(|kind| matches!(kind, 0 | 0o040000))
            } else {
                file_type != Some(0o040000)
            },
            "The capture contains an invalid directory entry."
        );
        total = total
            .checked_add(entry.size())
            .context("Invalid archive size.")?;
        ensure!(
            total <= MAX_CAPTURE_BYTES,
            "The expanded capture exceeds the 1 GiB limit."
        );
    }
    let manifest: Manifest = {
        let mut entry = archive
            .by_name("manifest.json")
            .context("The capture has no manifest.json.")?;
        ensure!(
            entry.size() <= MAX_MANIFEST_BYTES,
            "The capture manifest is too large."
        );
        let mut bytes = Vec::new();
        copy_hashed(&mut entry, &mut bytes, MAX_MANIFEST_BYTES)?;
        serde_json::from_slice(&bytes).context("The capture manifest is invalid.")?
    };
    ensure!(
        matches!(manifest.version, 1..=4),
        "This capture format is not supported."
    );
    ensure!(
        !manifest.capture_id.trim().is_empty() && manifest.capture_id.len() <= 128,
        "The capture identifier is invalid."
    );
    let captured_at = OffsetDateTime::parse(&manifest.captured_at, &Rfc3339)
        .context("The capture date must be an ISO-8601 timestamp.")?
        .to_offset(UtcOffset::UTC);
    // Match the library's fixed-width UTC timestamp contract so lexical date
    // sorting does not depend on the precision or timezone used by Shortcuts.
    let saved_at = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        captured_at.year(),
        captured_at.month() as u8,
        captured_at.day(),
        captured_at.hour(),
        captured_at.minute(),
        captured_at.second(),
        captured_at.millisecond()
    );
    if manifest.version == 2 {
        ensure!(
            names.len() == 1
                && manifest.attachments.is_empty()
                && manifest.text.is_none()
                && manifest.article.is_none(),
            "Instagram requests cannot contain other payloads."
        );
        let request = crate::instagram::InstagramRequest::parse(
            manifest
                .instagram_url
                .as_deref()
                .context("Missing Instagram media request.")?,
        )?;
        let resolver = resolver.context(
            "This Instagram share needs the Mac downloader. Open it in an updated Óia app.",
        )?;
        let media = resolver(&request)?;
        outcomes.push(import_instagram_media(
            library,
            &request,
            &media,
            Some(&saved_at),
        )?);
        return Ok(());
    }
    ensure!(
        manifest.instagram_url.is_none(),
        "Instagram requests require capture version 2."
    );
    let origin_url = manifest.origin.url()?;
    // Validate the whole typed manifest before importing any of its payloads.
    // A mismatched type must never save one item and discard the rest.
    validate_capture_type(&manifest, origin_url.as_deref())?;
    ensure!(
        manifest.attachments.len() <= 64,
        "The capture contains too many attachments."
    );
    let mut attachments = Vec::new();
    let mut referenced = HashSet::new();
    let article_html = if let Some(article) = &manifest.article {
        let file = extract_capture_attachment(
            &mut archive,
            &article.attachment,
            crate::article_html::MAX_ARTICLE_HTML_BYTES as u64,
            &mut referenced,
        )?;
        match article.format {
            ArticleFormat::Html => {
                Some(fs::read_to_string(file.path()).context("The article HTML must use UTF-8.")?)
            }
        }
    } else {
        None
    };
    // Validate and extract every payload to private, generated filenames before
    // importing anything. ZIP paths are never passed to filesystem writes.
    let mut attachment_bytes = 0_u64;
    for attachment in &manifest.attachments {
        let limit = if manifest.article.is_some() {
            MAX_SMALL_BYTES.saturating_sub(attachment_bytes)
        } else {
            MAX_BROWSER_VIDEO_BYTES
        };
        let file = extract_capture_attachment(&mut archive, attachment, limit, &mut referenced)?;
        if manifest.article.is_some() {
            attachment_bytes = attachment_bytes
                .checked_add(file.as_file().metadata()?.len())
                .context("Invalid article image size.")?;
            ensure!(
                attachment_bytes <= MAX_SMALL_BYTES,
                "Article images exceed 40 MiB."
            );
        }
        attachments.push((file, attachment));
    }
    // Unreferenced regular data is rejected rather than silently discarded when
    // the archive is removed after success (harmless directory entries are OK).
    ensure!(
        names.iter().all(|name| name == "manifest.json"
            || name.ends_with('/')
            || referenced.contains(name)),
        "The capture contains files that are not described in its manifest."
    );
    ensure!(
        !attachments.is_empty()
            || manifest
                .text
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
            || origin_url.is_some(),
        "The capture contains no supported content."
    );
    if manifest.version == 4
        && matches!(
            manifest.capture_type,
            Some(CaptureType::Article | CaptureType::Selection)
        )
    {
        let url = origin_url
            .or_else(|| {
                manifest
                    .text
                    .as_deref()
                    .and_then(|text| http_url(text).ok())
            })
            .context("The article capture has no source URL.")?;
        let mut images = Vec::new();
        let mut image_bytes = 0_u64;
        for (file, attachment) in &attachments {
            let source = attachment
                .source_url
                .as_deref()
                .context("An article image requires its source_url.")?;
            let source = url::Url::parse(&url)?.join(source)?;
            ensure!(
                matches!(source.scheme(), "http" | "https"),
                "An article image source must be an HTTP(S) URL."
            );
            let bytes = fs::read(file.path())?;
            image_bytes = image_bytes
                .checked_add(bytes.len() as u64)
                .context("Invalid article image size.")?;
            ensure!(
                image_bytes <= MAX_SMALL_BYTES,
                "Article images exceed 40 MiB."
            );
            let actual_type =
                image_type(&bytes).context("An article image is unsupported or incomplete.")?;
            ensure!(
                attachment
                    .content_type
                    .as_deref()
                    .is_none_or(|expected| expected.split(';').next() == Some(actual_type)),
                "An article image has an unexpected content type."
            );
            images.push(crate::ImageBytes {
                url: source.to_string(),
                content_type: actual_type.into(),
                bytes,
            });
        }
        // Decode and validate the entire payload before any library mutation.
        let article = article_html
            .as_deref()
            .map(|html| crate::article_html::parse_article_html(html, &url))
            .transpose()?;
        if manifest.capture_type == Some(CaptureType::Selection) {
            let text = manifest
                .text
                .as_deref()
                .context("The selection has no text.")?;
            if selection_kind(text, None) == SelectionKind::Quote {
                outcomes.push(import_text_capture(
                    library,
                    text,
                    &manifest.origin,
                    Some(&saved_at),
                    Some(CaptureType::Selection),
                    url_saver,
                )?);
            } else {
                let outcome = import_article_capture(
                    library,
                    &url,
                    &manifest.origin,
                    &saved_at,
                    ArticleContent {
                        html: article,
                        images,
                    },
                    url_saver,
                    article_fetcher,
                )?;
                let origin = Origin {
                    url: Some(crate::read_metadata(&library.article_path(&outcome.id))?.url),
                    title: manifest.origin.title.clone(),
                    canonical_url: manifest.origin.canonical_url.clone(),
                    site_name: manifest.origin.site_name.clone(),
                };
                outcomes.push(import_text_capture(
                    library,
                    text,
                    &origin,
                    Some(&saved_at),
                    Some(CaptureType::Selection),
                    url_saver,
                )?);
            }
        } else {
            outcomes.push(import_article_capture(
                library,
                &url,
                &manifest.origin,
                &saved_at,
                ArticleContent {
                    html: article,
                    images,
                },
                url_saver,
                article_fetcher,
            )?);
        }
        return Ok(());
    }
    for (file, attachment) in attachments {
        outcomes.push(import_payload(
            library,
            file.path(),
            &attachment.path,
            &manifest.origin,
            Some(&saved_at),
            url_saver,
        )?);
    }
    if let Some(text) = manifest
        .text
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        outcomes.push(import_text_capture(
            library,
            text,
            &manifest.origin,
            Some(&saved_at),
            (manifest.version >= 3)
                .then_some(manifest.capture_type)
                .flatten(),
            url_saver,
        )?);
    } else if outcomes.is_empty() {
        outcomes.push(import_text_capture(
            library,
            "",
            &manifest.origin,
            Some(&saved_at),
            (manifest.version >= 3)
                .then_some(manifest.capture_type)
                .flatten(),
            url_saver,
        )?);
    }
    Ok(())
}

pub(crate) fn import_instagram_media(
    library: &LibraryRoot,
    request: &crate::instagram::InstagramRequest,
    media: &crate::instagram::DownloadedMedia,
    saved_at: Option<&str>,
) -> Result<SaveOutcome> {
    ensure!(
        matches!(media.name.as_str(), "payload.jpg" | "payload.mp4"),
        "Unexpected Instagram payload."
    );
    let origin = Origin {
        url: Some(request.origin.clone()),
        title: Some(format!(
            "Instagram · {} · slide {}",
            request.shortcode, request.slide
        )),
        ..Origin::default()
    };
    import_payload(
        library,
        &media.path(),
        &media.name,
        &origin,
        saved_at,
        &|library, request| Ok(crate::save_url(library, request)?),
    )
}

fn import_payload(
    library: &LibraryRoot,
    path: &Path,
    name: &str,
    origin: &Origin,
    saved_at: Option<&str>,
    url_saver: &UrlSaver<'_>,
) -> Result<SaveOutcome> {
    let extension = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut file = open_regular_file(path)?;
    let size = file.metadata()?.len();
    let fallback = Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Saved item");
    let options = ImportOptions {
        title: Some(origin.title(fallback)),
        saved_at: saved_at.map(str::to_string),
        ..ImportOptions::default()
    };
    let outcome = match extension.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" => {
            ensure!(size <= MAX_SMALL_BYTES, "Images must be 40 MiB or smaller.");
            let mut bytes = Vec::new();
            copy_hashed(&mut file, &mut bytes, MAX_SMALL_BYTES)?;
            let content_type =
                image_type(&bytes).context("The image is unsupported or incomplete.")?;
            let outcome = if let Some(url) = origin.url()? {
                let extension = crate::images::image_extension(content_type, "");
                let media = format!(
                    "cuttings-asset:assets/{}.{extension}",
                    crate::sha256_hex(&bytes)
                );
                import_reading(
                    library,
                    SaveInput {
                        quote_identity_markdown: None,
                        kind: ReadingKind::Image,
                        lightweight: false,
                        url: url.clone(),
                        media_url: Some(media.clone()),
                        canonical_url: origin.canonical_url(&url),
                        title: origin.title(fallback),
                        author: None,
                        site: origin.site(&url),
                        saved_at: saved_at
                            .map(str::to_string)
                            .unwrap_or_else(crate::time::now_utc_iso),
                        markdown: format!("![Imported image]({media})"),
                        images: vec![crate::ImageBytes {
                            url: media,
                            content_type: content_type.to_string(),
                            bytes: bytes.clone(),
                        }],
                        preview_url: None,
                        favicon_url: None,
                        theme_color: None,
                        product: None,
                        link_category: None,
                        excerpt: None,
                        word_count: None,
                        lang: None,
                    },
                    ImportedReadingState::default(),
                )?
            } else {
                import_image_with_options(library, bytes.clone(), content_type, options)?
            };
            verify_saved(
                library,
                &outcome,
                SavedContent::MediaHash(&crate::sha256_hex(&bytes)),
            )?;
            outcome
        }
        "mp4" | "mov" | "m4v" => {
            ensure!(
                size <= MAX_BROWSER_VIDEO_BYTES,
                "Videos must be 1 GiB or smaller."
            );
            ensure!(
                complete_iso_boxes(&mut file)?
                    && crate::media_dimensions::video_dimensions(&mut file).is_some(),
                "The video is unsupported or incomplete."
            );
            file.rewind()?;
            let (_, hash) = copy_hashed(&mut file, &mut io::sink(), MAX_BROWSER_VIDEO_BYTES)?;
            let content_type = if extension == "mov" {
                "video/quicktime"
            } else {
                "video/mp4"
            };
            let outcome = if let Some(url) = origin.url()? {
                let mut import = begin_browser_video_import(
                    library,
                    BrowserVideoImportInput {
                        content_type: content_type.to_string(),
                        expected_bytes: Some(size),
                        origin_url: url.clone(),
                        canonical_url: origin.canonical_url(&url),
                        title: origin.title(fallback),
                        author: None,
                        site: origin.site(&url),
                        theme_color: None,
                        lang: None,
                        excerpt: None,
                        word_count: None,
                        saved_at: saved_at
                            .map(str::to_string)
                            .unwrap_or_else(crate::time::now_utc_iso),
                    },
                )?;
                file.rewind()?;
                let mut buffer = [0_u8; 256 * 1024];
                loop {
                    let count = file.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    import.append(&buffer[..count])?;
                }
                import.finish()?
            } else {
                import_video_file_with_options(library, path, content_type, options)?
            };
            verify_saved(library, &outcome, SavedContent::MediaHash(&hash))?;
            outcome
        }
        "txt" | "md" | "text" | "url" | "webloc" => {
            ensure!(
                size <= MAX_SMALL_BYTES,
                "Text files must be 40 MiB or smaller."
            );
            let mut bytes = Vec::new();
            copy_hashed(&mut file, &mut bytes, MAX_SMALL_BYTES)?;
            let text = if extension == "webloc" {
                let value = plist::Value::from_reader(Cursor::new(bytes))
                    .context("The web link file is invalid.")?;
                value
                    .as_dictionary()
                    .and_then(|dict| dict.get("URL"))
                    .and_then(plist::Value::as_string)
                    .context("The web link file has no URL.")?
                    .to_string()
            } else {
                let text = String::from_utf8(bytes).context("Text must use UTF-8 encoding.")?;
                if extension == "url" {
                    text.lines()
                        .find_map(|line| {
                            line.split_once('=')
                                .filter(|(key, _)| key.trim().eq_ignore_ascii_case("URL"))
                                .map(|(_, value)| value.trim().to_string())
                        })
                        .context("The internet shortcut file has no URL.")?
                } else {
                    text
                }
            };
            if extension == "url" || extension == "webloc" {
                http_url(&text)?;
            }
            import_text_capture(library, &text, origin, saved_at, None, url_saver)?
        }
        _ => bail!("This file type is not supported. Use an image, video, UTF-8 text, or link."),
    };
    Ok(outcome)
}

fn article_kind(article: &crate::article_html::ArticleHtml) -> crate::PageCaptureKind {
    crate::classify_page_capture(
        &article.page_url,
        &article.structured_html,
        &article.markdown,
        Some(&article.evidence),
    )
}

struct ArticleContent {
    html: Option<crate::article_html::ArticleHtml>,
    images: Vec<crate::ImageBytes>,
}

fn source_retrieval_failure(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<crate::SaveUrlError>(),
        Some(
            crate::SaveUrlError::Retrieval(_)
                | crate::SaveUrlError::Save(crate::SaveError::InvalidRequest(_))
        )
    )
}

fn validate_article_images(images: &[crate::ImageBytes]) -> Result<()> {
    let total: usize = images.iter().map(|image| image.bytes.len()).sum();
    ensure!(
        total <= MAX_SMALL_BYTES as usize,
        "Article images exceed 40 MiB."
    );
    let mut sources = std::collections::HashMap::new();
    for image in images {
        let hash = crate::sha256_hex(&image.bytes);
        ensure!(
            sources
                .insert(&image.url, hash.clone())
                .is_none_or(|previous| previous == hash),
            "An article image source has conflicting contents."
        );
    }
    Ok(())
}

/// A version-4 article intent may include live HTML or ask for a bounded public
/// retrieval. Earlier URL ingress remains the shared lightweight URL save.
fn import_article_capture(
    library: &LibraryRoot,
    url: &str,
    origin: &Origin,
    saved_at: &str,
    content: ArticleContent,
    url_saver: &UrlSaver<'_>,
    article_fetcher: &ArticleFetcher<'_>,
) -> Result<SaveOutcome> {
    let ArticleContent {
        html: mut article,
        mut images,
    } = content;
    if let Some(article) = &article {
        let detected = crate::parse_link_metadata(&article.structured_html, &article.page_url);
        ensure!(
            images.iter().all(|image| {
                article.markdown.contains(&image.url)
                    || detected
                        .as_ref()
                        .and_then(|metadata| metadata.image_url.as_deref())
                        == Some(image.url.as_str())
            }),
            "An article contains an unrelated image attachment."
        );
    }
    if let Some(article) = &mut article {
        images.append(&mut article.images);
    }
    validate_article_images(&images)?;
    let resolved = match url_saver(library, origin_url_request(origin, url, saved_at)) {
        Ok(outcome) => {
            verify_saved(library, &outcome, SavedContent::Link)?;
            let metadata = crate::read_metadata(&library.article_path(&outcome.id))?;
            // Existing complete captures and source-specific adapters own their
            // reading. A later iPhone snapshot never replaces those bodies.
            if !metadata.lightweight || metadata.source_profile.is_some() {
                return Ok(outcome);
            }
            if article.is_none() && (metadata.link_category.is_some() || metadata.product.is_some())
            {
                return Ok(outcome);
            }
            Some(outcome)
        }
        Err(error)
            if source_retrieval_failure(&error)
                && article.as_ref().is_some_and(|article| {
                    article_kind(article) == crate::PageCaptureKind::Article
                }) =>
        {
            None
        }
        Err(error) => return Err(error),
    };
    if article
        .as_ref()
        .is_none_or(|value| value.markdown.trim().is_empty())
    {
        // Fetch failure and weak extraction are ordinary fallbacks. The URL
        // reading already exists, so the share stays useful while offline.
        article = article_fetcher(url)
            .ok()
            .and_then(|page| crate::article_html::parse_article_html(&page.html, &page.url).ok());
    }
    let Some(mut article) = article else {
        return resolved.context("No complete article could be captured.");
    };
    images.append(&mut article.images);
    validate_article_images(&images)?;
    let stored = resolved
        .as_ref()
        .map(|outcome| crate::read_metadata(&library.article_path(&outcome.id)))
        .transpose()?;
    let target_url = stored
        .as_ref()
        .map(|metadata| metadata.url.clone())
        .unwrap_or_else(|| url.to_string());
    let full = article_kind(&article) == crate::PageCaptureKind::Article
        && stored
            .as_ref()
            .is_none_or(|metadata| metadata.link_category.is_none() && metadata.product.is_none());
    let detected = crate::parse_link_metadata(&article.structured_html, &article.page_url);
    let category = detected
        .as_ref()
        .map(|metadata| metadata.category)
        .or_else(|| {
            stored
                .as_ref()
                .and_then(|metadata| metadata.link_category.as_deref())
                .and_then(crate::LinkCategory::parse)
        });
    let product = crate::parse_product_metadata(&article.structured_html, &article.page_url)
        .or_else(|| {
            stored
                .as_ref()
                .and_then(|metadata| metadata.product.clone())
        });
    let title = detected
        .as_ref()
        .and_then(|metadata| metadata.title.clone())
        .or_else(|| origin.title.clone())
        .or(article.title)
        .unwrap_or_else(|| target_url.clone());
    let canonical_url = origin
        .canonical_url
        .as_deref()
        .and_then(|url| http_url(url).ok())
        .or(article.canonical_url)
        .unwrap_or_else(|| target_url.clone());
    let site = origin
        .site_name
        .clone()
        .or(article.site)
        .or_else(|| origin.site(&target_url));
    let preview_url = detected
        .as_ref()
        .and_then(|metadata| metadata.image_url.clone())
        .filter(|url| images.iter().any(|image| &image.url == url))
        .or_else(|| images.first().map(|image| image.url.clone()));
    let markdown = if full {
        article.markdown
    } else {
        format!("[Open link](<{target_url}>)")
    };
    let outcome = if full {
        import_reading(
            library,
            SaveInput {
                quote_identity_markdown: None,
                kind: ReadingKind::Article,
                lightweight: false,
                url: target_url.clone(),
                media_url: None,
                canonical_url,
                title,
                author: None,
                site,
                saved_at: saved_at.into(),
                markdown: markdown.clone(),
                images: images
                    .iter()
                    .map(|image| crate::ImageBytes {
                        url: image.url.clone(),
                        content_type: image.content_type.clone(),
                        bytes: image.bytes.clone(),
                    })
                    .collect(),
                preview_url,
                favicon_url: None,
                theme_color: None,
                product,
                link_category: category,
                excerpt: None,
                word_count: Some(markdown.split_whitespace().count().min(u32::MAX as usize) as u32),
                lang: None,
            },
            ImportedReadingState::default(),
        )?
    } else {
        let Some(resolved) = resolved else {
            bail!("No complete article could be captured.");
        };
        let input = crate::SaveLinkInput {
            url: target_url.clone(),
            canonical_url,
            title,
            author: None,
            site,
            saved_at: saved_at.into(),
            images: images
                .iter()
                .map(|image| crate::ImageBytes {
                    url: image.url.clone(),
                    content_type: image.content_type.clone(),
                    bytes: image.bytes.clone(),
                })
                .collect(),
            preview_url,
            favicon_url: None,
            theme_color: None,
            product,
            link_category: category,
            excerpt: None,
            lang: None,
        };
        let snapshot = fs::read(library.article_path(&resolved.id))?;
        crate::import_link_capture_if_unchanged(
            library,
            input,
            ImportedReadingState::default(),
            &crate::sha256_hex(&snapshot),
        )?
    };
    verify_saved(
        library,
        &outcome,
        SavedContent::Page {
            url: &target_url,
            markdown: &markdown,
            images: &images,
            full,
        },
    )?;
    Ok(outcome)
}

fn import_text_capture(
    library: &LibraryRoot,
    text: &str,
    origin: &Origin,
    saved_at: Option<&str>,
    capture_type: Option<CaptureType>,
    url_saver: &UrlSaver<'_>,
) -> Result<SaveOutcome> {
    ensure!(
        text.len() as u64 <= MAX_SMALL_BYTES,
        "Text must be 40 MiB or smaller."
    );
    let saved_at = saved_at
        .map(str::to_string)
        .unwrap_or_else(crate::time::now_utc_iso);
    let text = text.trim().trim_start_matches('\u{feff}');
    let origin_url = origin.url()?;
    let text_url = http_url(text).ok();
    let is_link = capture_type != Some(CaptureType::Selection)
        && match &origin_url {
            Some(url) => text.is_empty() || text_url.as_ref() == Some(url),
            None => text_url.is_some(),
        };
    if is_link {
        let url = origin_url
            .or(text_url)
            .context("A URL capture has no URL.")?;
        let outcome = url_saver(library, origin_url_request(origin, &url, &saved_at))?;
        verify_saved(library, &outcome, SavedContent::Link)?;
        return Ok(outcome);
    }
    let outcome = if let Some(mut url) = origin_url {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        ensure!(
            text.len() <= crate::MAX_SELECTION_TEXT_BYTES,
            "A selection must contain 1–65536 bytes of text."
        );
        let kind = selection_kind(&text, None);
        if kind == SelectionKind::Highlight {
            // Resolve the same URL category/source as any other ingress before
            // saving its passage. A provider alias can select an existing
            // canonical identity; attach to that reading rather than making
            // another article at the submitted alias URL.
            let resolved = url_saver(library, origin_url_request(origin, &url, &saved_at))?;
            verify_saved(library, &resolved, SavedContent::Link)?;
            url = crate::read_metadata(&library.article_path(&resolved.id))?.url;
        }
        // No DOM is available in the Shortcut. Core interprets enclosing quote
        // marks; all other source-backed text is a highlight. A lightweight
        // placeholder cannot replace a previously captured article body.
        let outcome = save_selection(
            library,
            SelectionSaveInput {
                capture: SaveInput {
                    quote_identity_markdown: None,
                    kind: ReadingKind::Article,
                    lightweight: true,
                    url: url.clone(),
                    media_url: None,
                    canonical_url: origin.canonical_url(&url),
                    title: origin.title(&url),
                    author: None,
                    site: origin.site(&url),
                    saved_at,
                    markdown: String::new(),
                    images: vec![],
                    preview_url: None,
                    favicon_url: None,
                    theme_color: None,
                    product: None,
                    link_category: None,
                    excerpt: None,
                    word_count: None,
                    lang: None,
                },
                selected_text: text.clone(),
                context: None,
            },
        )?;
        verify_saved(
            library,
            &outcome,
            if kind == SelectionKind::Quote {
                SavedContent::QuoteText(&text)
            } else {
                SavedContent::Highlight {
                    text: &text,
                    url: &url,
                }
            },
        )?;
        outcome
    } else {
        let outcome = import_text_with_options(
            library,
            text,
            ImportOptions {
                title: origin.title.clone(),
                saved_at: Some(saved_at),
                ..ImportOptions::default()
            },
        )?;
        verify_saved(library, &outcome, SavedContent::QuoteText(text))?;
        outcome
    };
    Ok(outcome)
}

fn origin_url_request(origin: &Origin, url: &str, saved_at: &str) -> UrlSaveRequest {
    UrlSaveRequest {
        url: url.to_string(),
        title_hint: origin.title.clone(),
        canonical_url_hint: Some(origin.canonical_url(url)),
        site_hint: origin.site(url),
        saved_at: Some(saved_at.to_string()),
    }
}

fn http_url(value: &str) -> Result<String> {
    let value = value.trim();
    ensure!(
        !value.chars().any(char::is_whitespace),
        "The source must be one HTTP(S) URL."
    );
    let parsed = url::Url::parse(value)?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some(),
        "The source must be an HTTP(S) URL."
    );
    crate::normalize_url(value)
}

pub(crate) fn image_type(bytes: &[u8]) -> Option<&'static str> {
    let content_type =
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.ends_with(b"\0\0\0\0IEND\xaeB`\x82") {
            "image/png"
        } else if bytes.starts_with(b"\xff\xd8\xff") && bytes.ends_with(b"\xff\xd9") {
            "image/jpeg"
        } else if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
            && bytes.ends_with(b";")
        {
            "image/gif"
        } else if bytes.len() >= 12
            && &bytes[..4] == b"RIFF"
            && &bytes[8..12] == b"WEBP"
            && u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize + 8 == bytes.len()
        {
            "image/webp"
        } else if bytes.len() >= 12
            && &bytes[4..8] == b"ftyp"
            && complete_iso_boxes(&mut Cursor::new(bytes)).ok()?
        {
            match &bytes[8..12] {
                b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => "image/heic",
                _ => return None,
            }
        } else {
            return None;
        };
    crate::media_dimensions::image_dimensions(&mut Cursor::new(bytes))?;
    Some(content_type)
}

fn complete_iso_boxes(reader: &mut (impl Read + Seek)) -> Result<bool> {
    let end = reader.seek(SeekFrom::End(0))?;
    let mut offset = 0_u64;
    let mut boxes = 0;
    while offset < end {
        if end - offset < 8 {
            return Ok(false);
        }
        reader.seek(SeekFrom::Start(offset))?;
        let mut header = [0_u8; 8];
        reader.read_exact(&mut header)?;
        let size = u32::from_be_bytes(header[..4].try_into().unwrap());
        let (size, minimum) = match size {
            0 => (end - offset, 8),
            1 => {
                let mut bytes = [0_u8; 8];
                reader.read_exact(&mut bytes)?;
                (u64::from_be_bytes(bytes), 16)
            }
            size => (size as u64, 8),
        };
        if size < minimum || size > end - offset {
            return Ok(false);
        }
        offset += size;
        boxes += 1;
        if boxes > 1_000_000 {
            return Ok(false);
        }
    }
    Ok(boxes > 0)
}

enum SavedContent<'a> {
    MediaHash(&'a str),
    QuoteText(&'a str),
    Highlight {
        text: &'a str,
        url: &'a str,
    },
    Page {
        url: &'a str,
        markdown: &'a str,
        images: &'a [crate::ImageBytes],
        full: bool,
    },
    Link,
}

fn verify_saved(
    library: &LibraryRoot,
    outcome: &SaveOutcome,
    expected: SavedContent<'_>,
) -> Result<()> {
    // Existing readings may be corrupt or partially synchronized. A duplicate
    // ID alone is not permission to discard the only complete Inbox copy.
    let article = library.article_path(&outcome.id);
    for directory in [
        library.articles_dir(),
        library
            .reading_dir(&outcome.id)
            .parent()
            .unwrap()
            .to_path_buf(),
        library.reading_dir(&outcome.id),
    ] {
        ensure!(
            fs::symlink_metadata(directory)?.file_type().is_dir(),
            "The saved reading has an unsafe directory."
        );
    }
    let mut file = open_regular_file(&article)?;
    let mut content = Vec::new();
    copy_hashed(&mut file, &mut content, MAX_SMALL_BYTES)?;
    let reading = parse_reading(std::str::from_utf8(&content)?)?;
    ensure!(
        reading.metadata.id == outcome.id,
        "The saved reading has an unexpected identity."
    );
    match expected {
        SavedContent::QuoteText(text) => {
            // Quote IDs outlive user edits and can also survive a truncated
            // body with valid frontmatter. Never consume the original text
            // just because a file still carries its old ID. Remove exactly
            // the wrapper added by import_text/import_text_capture; literal
            // '>' characters in the selection remain part of its identity.
            let body = reading
                .body
                .trim_end_matches(['\r', '\n'])
                .lines()
                .map(|line| {
                    let line = line.strip_prefix('>')?;
                    Some(line.strip_prefix(' ').unwrap_or(line))
                })
                .collect::<Option<Vec<_>>>()
                .map(|lines| lines.join("\n"));
            ensure!(
                reading.metadata.kind == ReadingKind::Quote
                    && body.as_ref().is_some_and(|body| {
                        body.split_whitespace().eq(text.split_whitespace())
                    })
                    && crate::quote_id(&reading.metadata.url, text)
                        .is_ok_and(|id| id == outcome.id),
                "The saved quote has different or incomplete text. The Inbox copy was kept."
            );
        }
        SavedContent::Link => {
            // A full article may already occupy this link's URL-derived ID.
            // Its title and body can be user-edited; the retained URL is the
            // content this lightweight capture needs to preserve.
            ensure!(
                reading.metadata.kind == ReadingKind::Article
                    && crate::url_id(&reading.metadata.url).is_ok_and(|id| id == outcome.id),
                "The saved link has an unexpected source. The Inbox copy was kept."
            );
        }
        SavedContent::Highlight { text, url } => {
            ensure!(
                reading.metadata.kind == ReadingKind::Article
                    && reading.metadata.url == url
                    && crate::url_id(url).is_ok_and(|id| id == outcome.id),
                "The saved highlight has an unexpected source. The Inbox copy was kept."
            );
            let mut sidecar = open_regular_file(&library.highlights_path(&outcome.id))
                .context("The saved highlights are unavailable. The Inbox copy was kept.")?;
            let before = sidecar.metadata()?;
            let highlights = crate::list_highlights(library, &outcome.id)?;
            ensure!(
                highlights.iter().any(|highlight| {
                    highlight.text == text.trim()
                        && reading.metadata.featured_highlight_id.as_deref()
                            == Some(highlight.id.as_str())
                }) && same_file_contents(
                    &before,
                    &fs::symlink_metadata(library.highlights_path(&outcome.id))?
                ),
                "The saved highlight has different or incomplete text. The Inbox copy was kept."
            );
            // Pin and flush the sidecar as well as article.md before allowing
            // the at-least-once archive to disappear.
            copy_hashed(&mut sidecar, &mut io::sink(), MAX_SMALL_BYTES)?;
            sidecar.sync_all()?;
        }
        SavedContent::Page {
            url,
            markdown,
            images,
            full,
        } => {
            ensure!(reading.metadata.kind == ReadingKind::Article
                && reading.metadata.url == url
                && crate::url_id(url).is_ok_and(|id| id == outcome.id)
                && (!full || !reading.metadata.lightweight),
                "The saved article has an unexpected source or incomplete body. The Inbox copy was kept.");
            // Full duplicates can contain user-edited text. The existing full
            // reading is authoritative, matching an extension article re-save.
            if outcome.disposition != SaveDisposition::Duplicate {
                let mut expected = markdown.to_string();
                for image in images {
                    let hash = crate::sha256_hex(&image.bytes);
                    let filename = format!(
                        "{hash}.{}",
                        crate::images::image_extension(&image.content_type, &image.url)
                    );
                    expected = crate::images::rewrite_markdown_image_destinations(
                        &expected,
                        &image.url,
                        &format!("assets/{filename}"),
                    );
                    let mut asset =
                        open_regular_file(&library.assets_dir(&outcome.id).join(&filename))?;
                    let (_, actual) = copy_hashed(&mut asset, &mut io::sink(), MAX_SMALL_BYTES)?;
                    ensure!(
                        actual == hash,
                        "The saved article image is incomplete. The Inbox copy was kept."
                    );
                    asset.sync_all()?;
                }
                ensure!(
                    reading.body.trim_end_matches(['\r', '\n'])
                        == expected.trim_end_matches(['\r', '\n']),
                    "The saved article has different or incomplete text. The Inbox copy was kept."
                );
            }
        }
        SavedContent::MediaHash(hash) => {
            let valid_identity = if reading.metadata.kind == ReadingKind::Video
                && reading.metadata.url == format!("cuttings://local/video/{hash}")
            {
                // Source-less videos use the shared importer's byte-derived
                // identity rather than a synthetic-origin media_id.
                outcome.id == crate::sha256_hex(format!("video\0{hash}").as_bytes())
            } else {
                reading.metadata.kind.is_media()
                    && reading.metadata.media_url.as_deref().is_some_and(|media| {
                        crate::media_id(reading.metadata.kind, &reading.metadata.url, media)
                            .is_ok_and(|id| id == outcome.id)
                    })
            };
            ensure!(
                valid_identity,
                "The saved media has an unexpected source. The Inbox copy was kept."
            );
        }
    }
    file.sync_all()?;
    if let SavedContent::MediaHash(expected_hash) = expected {
        let relative = match reading.metadata.kind {
            ReadingKind::Image => crate::first_local_image_asset(&reading.body),
            ReadingKind::Video => reading
                .metadata
                .media_url
                .as_deref()
                .and_then(|url| url.strip_prefix("cuttings-asset:"))
                .map(str::to_string),
            _ => None,
        }
        .context("The saved reading does not contain its media asset. The Inbox copy was kept.")?;
        let filename = relative
            .strip_prefix("assets/")
            .filter(|name| {
                !name.is_empty()
                    && !name.contains('/')
                    && !name.contains('\\')
                    && *name != "."
                    && *name != ".."
            })
            .context("The saved media asset path is unsafe.")?;
        ensure!(
            fs::symlink_metadata(library.assets_dir(&outcome.id))?
                .file_type()
                .is_dir(),
            "The saved asset directory is unsafe."
        );
        let mut asset = open_regular_file(&library.assets_dir(&outcome.id).join(filename))
            .context("The saved media asset is unavailable. The Inbox copy was kept.")?;
        let (_, actual) = copy_hashed(&mut asset, &mut io::sink(), MAX_BROWSER_VIDEO_BYTES)?;
        ensure!(
            actual == expected_hash,
            "The saved media asset has different contents. The Inbox copy was kept."
        );
        asset.sync_all()?;
    }
    // Flush directory entries from the asset upward before removing the only
    // input. A successful rename alone is not a durable commit after a crash.
    for directory in [
        library.assets_dir(&outcome.id),
        library.reading_dir(&outcome.id),
        library
            .reading_dir(&outcome.id)
            .parent()
            .unwrap()
            .to_path_buf(),
        library.articles_dir(),
        library.path().to_path_buf(),
    ] {
        open_regular_directory(&directory)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn library() -> (TempDir, LibraryRoot) {
        let temp = TempDir::new().unwrap();
        let library = LibraryRoot::new(temp.path()).unwrap();
        fs::create_dir(library.inbox_dir()).unwrap();
        (temp, library)
    }

    fn process(library: &LibraryRoot) -> InboxReport {
        process_inbox_with_fetchers_at(
            library,
            &[],
            SystemTime::now() + Duration::from_secs(60),
            None,
            &save_offline_url,
            &unavailable_article,
        )
        .unwrap()
    }

    fn unavailable_article(_: &str) -> Result<FetchedArticle> {
        bail!("fixture article retrieval unavailable")
    }

    fn save_offline_url(library: &LibraryRoot, request: UrlSaveRequest) -> Result<SaveOutcome> {
        Ok(crate::save_link_capture(
            library,
            crate::SaveLinkInput {
                canonical_url: request
                    .canonical_url_hint
                    .unwrap_or_else(|| request.url.clone()),
                title: request.title_hint.unwrap_or_else(|| request.url.clone()),
                author: None,
                site: request.site_hint,
                saved_at: request.saved_at.unwrap_or_else(crate::time::now_utc_iso),
                link_category: crate::parse_link_metadata("", &request.url)
                    .map(|metadata| metadata.category),
                url: request.url,
                images: vec![],
                preview_url: None,
                favicon_url: None,
                theme_color: None,
                product: None,
                excerpt: None,
                lang: None,
            },
        )?)
    }

    fn readings(library: &LibraryRoot) -> Vec<crate::Reading> {
        crate::scan_library(library)
            .unwrap()
            .into_iter()
            .map(|item| {
                parse_reading(&fs::read_to_string(library.article_path(&item.id)).unwrap()).unwrap()
            })
            .collect()
    }

    fn png() -> Vec<u8> {
        hex::decode("89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000b49444154789c636000020000050001a5f645400000000049454e44ae426082").unwrap()
    }

    fn movie() -> Vec<u8> {
        fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut bytes = ((body.len() + 8) as u32).to_be_bytes().to_vec();
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(body);
            bytes
        }
        let mut header = vec![0_u8; 40];
        for value in [0x0001_0000_i32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
            header.extend_from_slice(&value.to_be_bytes());
        }
        header.extend_from_slice(&(640_u32 << 16).to_be_bytes());
        header.extend_from_slice(&(480_u32 << 16).to_be_bytes());
        let mut handler = vec![0_u8; 8];
        handler.extend_from_slice(b"vide");
        let mut track = atom(b"tkhd", &header);
        track.extend_from_slice(&atom(b"mdia", &atom(b"hdlr", &handler)));
        let mut bytes = atom(b"ftyp", b"isom\0\0\0\0isommp42");
        bytes.extend_from_slice(&atom(b"moov", &atom(b"trak", &track)));
        bytes.extend_from_slice(&atom(b"mdat", &[0; 32]));
        bytes
    }

    fn manifest() -> serde_json::Value {
        json!({"version": 1, "capture_id": "test-capture", "captured_at": "2026-09-10T12:34:56+01:00"})
    }

    fn archive(
        library: &LibraryRoot,
        manifest: &serde_json::Value,
        entries: &[(&str, &[u8])],
    ) -> std::path::PathBuf {
        let path = library.inbox_dir().join("test.cuttingscapture.zip");
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        zip.start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(manifest).unwrap())
            .unwrap();
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn instagram_manifest() -> serde_json::Value {
        json!({"version": 2, "capture_id": "instagram-test", "captured_at": "2026-09-22T12:00:00Z",
            "instagram_url": "https://www.instagram.com/p/DdlVpikk5Gj/?img_index=2&stkn=tracking"})
    }

    fn article_manifest(html: &str) -> serde_json::Value {
        json!({"version":4, "capture_id":"article-test", "captured_at":"2026-10-11T12:00:00Z",
            "capture_type":"article", "origin":{"url":"https://example.com/article", "title":"Shared article"},
            "article":{"path":"article.html", "format":"html", "byte_count":html.len(), "sha256":crate::sha256_hex(html.as_bytes())}})
    }

    fn process_articles(library: &LibraryRoot, fetcher: &ArticleFetcher<'_>) -> InboxReport {
        process_inbox_with_fetchers_at(
            library,
            &[],
            SystemTime::now() + Duration::from_secs(60),
            None,
            &save_offline_url,
            fetcher,
        )
        .unwrap()
    }

    #[test]
    fn article_html_upgrades_url_identity_with_local_images_and_existing_state() {
        let (_temp, library) = library();
        let url = "https://example.com/article";
        let saved = import_text_capture(
            &library,
            "Earlier passage",
            &Origin {
                url: Some(url.into()),
                ..Origin::default()
            },
            None,
            Some(CaptureType::Selection),
            &save_offline_url,
        )
        .unwrap();
        let path = library.article_path(&saved.id);
        let mut before = parse_reading(&fs::read_to_string(&path).unwrap()).unwrap();
        before.metadata.tags = vec!["kept".into()];
        before.metadata.favorite = true;
        fs::write(&path, crate::render_reading(&before).unwrap()).unwrap();
        let html = "<meta property='og:type' content='article'><main><h1>Heading</h1><p>Full captured article body with <strong>important words</strong>.</p><img src='/image.png' alt='Artwork'></main>";
        let bytes = png();
        let mut manifest = article_manifest(html);
        manifest["attachments"] = json!([{"path":"image.png", "source_url":"/image.png", "content_type":"image/png", "byte_count":bytes.len(), "sha256":crate::sha256_hex(&bytes)}]);
        let archive_path = archive(
            &library,
            &manifest,
            &[("article.html", html.as_bytes()), ("image.png", &bytes)],
        );
        let fetcher = |_: &str| -> Result<FetchedArticle> {
            panic!("Transported article must not be fetched again")
        };
        let report = process_articles(&library, &fetcher);
        assert_eq!(report.saved, 1, "{report:?}");
        assert!(!archive_path.exists());
        let reading = parse_reading(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reading.metadata.id, crate::url_id(url).unwrap());
        assert!(!reading.metadata.lightweight);
        assert!(reading.body.contains("**important words**"));
        let asset = format!("assets/{}.png", crate::sha256_hex(&bytes));
        assert!(reading.body.contains(&asset));
        assert_eq!(
            fs::read(library.reading_dir(&saved.id).join(asset)).unwrap(),
            bytes
        );
        assert_eq!(reading.metadata.tags, before.metadata.tags);
        assert_eq!(reading.metadata.favorite, before.metadata.favorite);
        assert_eq!(reading.metadata.saved_at, before.metadata.saved_at);
        assert_eq!(
            reading.metadata.featured_highlight_id,
            before.metadata.featured_highlight_id
        );
        assert_eq!(
            crate::list_highlights(&library, &saved.id).unwrap()[0].text,
            "Earlier passage"
        );

        let changed_html = "<meta property='og:type' content='article'><article><p>A different resave body.</p></article>";
        let manifest = article_manifest(changed_html);
        let archive_path = archive(
            &library,
            &manifest,
            &[("article.html", changed_html.as_bytes())],
        );
        let report = process_articles(&library, &fetcher);
        assert_eq!(report.duplicates, 1, "{report:?}");
        assert!(!archive_path.exists());
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            crate::render_reading(&reading).unwrap()
        );
    }

    #[test]
    fn article_intent_without_payload_fetches_prose_for_origin_and_text_urls() {
        for text_url in [false, true] {
            let (_temp, library) = library();
            let mut manifest = article_manifest("");
            manifest.as_object_mut().unwrap().remove("article");
            if text_url {
                manifest.as_object_mut().unwrap().remove("origin");
                manifest["text"] = json!("https://example.com/article");
            }
            let path = archive(&library, &manifest, &[]);
            let calls = std::cell::Cell::new(0);
            let fetcher = |url: &str| {
                assert_eq!(url, "https://example.com/article");
                calls.set(calls.get() + 1);
                Ok(FetchedArticle {
                    html: format!(
                        "<main><p>{}</p><p>{}</p></main>",
                        "Useful article prose. ".repeat(20),
                        "More meaningful context. ".repeat(20)
                    ),
                    url: url.to_string(),
                })
            };
            let report = process_articles(&library, &fetcher);
            assert_eq!(report.saved, 1, "{report:?}");
            assert!(!path.exists());
            assert_eq!(calls.get(), 1);
            assert!(!readings(&library)[0].metadata.lightweight);
            assert!(readings(&library)[0].body.contains("Useful article prose."));
        }
    }

    #[test]
    fn redirected_article_resolves_relative_destinations_at_final_page_but_keeps_visited_identity()
    {
        let (_temp, library) = library();
        let mut manifest = article_manifest("");
        manifest.as_object_mut().unwrap().remove("article");
        manifest["origin"]["url"] = json!("https://example.com/redirect");
        archive(&library, &manifest, &[]);
        let fetcher = |_: &str| {
            Ok(FetchedArticle {
            url: "https://publisher.example/stories/final".into(),
            html: "<meta property='og:type' content='article'><meta property='og:url' content='https://publisher.example/stories/final'><link rel='canonical' href='final-canonical'><main><p>Full captured prose with <a href='related'>a relative link</a>.</p><img src='image.png' alt='Page image'></main>".into(),
        })
        };
        let report = process_articles(&library, &fetcher);
        assert_eq!(report.saved, 1, "{report:?}");
        let reading = readings(&library).pop().unwrap();
        assert_eq!(
            reading.metadata.id,
            crate::url_id("https://example.com/redirect").unwrap()
        );
        assert_eq!(reading.metadata.url, "https://example.com/redirect");
        assert_eq!(
            reading.metadata.canonical_url,
            "https://publisher.example/stories/final-canonical"
        );
        assert!(reading
            .body
            .contains("https://publisher.example/stories/related"));
        assert!(reading
            .body
            .contains("https://publisher.example/stories/image.png"));
        assert!(!reading.metadata.lightweight);
    }

    #[test]
    fn declared_article_attachment_size_is_rejected_before_extraction() {
        let (_temp, library) = library();
        let manifest = manifest();
        let path = archive(&library, &manifest, &[("image.png", b"oversized fixture")]);
        let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let attachment = Attachment {
            path: "image.png".into(),
            byte_count: None,
            sha256: None,
            source_url: None,
            content_type: None,
        };
        assert!(
            extract_capture_attachment(&mut archive, &attachment, 4, &mut HashSet::new()).is_err()
        );
    }

    #[test]
    fn unavailable_or_weak_article_extraction_keeps_a_useful_link() {
        for html in [
            None,
            Some("<main><p>Homepage labels</p><a href='/one'>One</a></main>"),
        ] {
            let (_temp, library) = library();
            let mut manifest = article_manifest("");
            manifest.as_object_mut().unwrap().remove("article");
            let path = archive(&library, &manifest, &[]);
            let fetcher = |url: &str| {
                html.map(|html| FetchedArticle {
                    html: html.into(),
                    url: url.into(),
                })
                .context("fixture offline")
            };
            let report = process_articles(&library, &fetcher);
            assert_eq!(report.saved, 1, "{report:?}");
            assert!(!path.exists());
            let reading = readings(&library).pop().unwrap();
            assert!(reading.metadata.lightweight);
            assert_eq!(reading.body, "[Open link](<https://example.com/article>)\n");
        }
    }

    #[test]
    fn inline_data_image_is_local_and_uncaptured_remote_image_is_a_placeholder() {
        use base64::Engine as _;
        let (_temp, library) = library();
        let bytes = png();
        let html = format!("<meta property='og:type' content='article'><article><p>Captured story.</p><img src='data:image/png;base64,{}' alt='Embedded'><img src='/unavailable.png' alt='Missing image'></article>", base64::engine::general_purpose::STANDARD.encode(&bytes));
        let manifest = article_manifest(&html);
        archive(&library, &manifest, &[("article.html", html.as_bytes())]);
        let report = process_articles(&library, &unavailable_article);
        assert_eq!(report.saved, 1, "{report:?}");
        let reading = readings(&library).pop().unwrap();
        assert!(!reading.body.contains("data:image"));
        assert!(reading
            .body
            .contains(&format!("assets/{}.png", crate::sha256_hex(&bytes))));
        assert!(reading
            .body
            .contains("![Missing image](<https://example.com/unavailable.png>)"));
    }

    #[test]
    fn article_selection_keeps_catalog_kind_and_quotes_skip_optional_retrieval() {
        for quote in [false, true] {
            let (_temp, library) = library();
            let html = format!("<script type='application/ld+json'>{{\"@type\":\"Movie\",\"name\":\"Movie title\",\"url\":\"https://example.com/article\"}}</script><main><p>{}</p><p>{}</p></main>", "Catalog description. ".repeat(20), "More catalog prose. ".repeat(20));
            let mut manifest = article_manifest(&html);
            manifest["capture_type"] = json!("selection");
            manifest["text"] = json!(if quote {
                "“Quoted passage”"
            } else {
                "Highlighted passage"
            });
            let path = archive(&library, &manifest, &[("article.html", html.as_bytes())]);
            let saver = |library: &LibraryRoot, request: UrlSaveRequest| {
                assert!(!quote, "Quote must remain offline");
                save_offline_url(library, request)
            };
            let fetcher = |_: &str| -> Result<FetchedArticle> {
                panic!("Supplied selection body must not be fetched")
            };
            let report = process_inbox_with_fetchers_at(
                &library,
                &[],
                SystemTime::now() + Duration::from_secs(60),
                None,
                &saver,
                &fetcher,
            )
            .unwrap();
            assert_eq!(report.saved, 1, "{report:?}");
            assert!(!path.exists());
            let reading = readings(&library).pop().unwrap();
            if quote {
                assert_eq!(reading.metadata.kind, ReadingKind::Quote);
            } else {
                assert!(reading.metadata.lightweight);
                assert_eq!(reading.metadata.link_category.as_deref(), Some("movie"));
                assert_eq!(
                    crate::list_highlights(&library, &reading.metadata.id).unwrap()[0].text,
                    "Highlighted passage"
                );
            }
        }
    }

    #[test]
    fn malformed_article_payloads_are_retained_before_any_library_write() {
        let html =
            "<meta property='og:type' content='article'><article><p>Captured story.</p></article>";
        for invalid in [
            "checksum",
            "size",
            "format",
            "collision",
            "type",
            "source",
            "utf8",
            "image-source",
        ] {
            let (_temp, library) = library();
            let mut manifest = article_manifest(html);
            let mut content = html.as_bytes().to_vec();
            let mut extra = Vec::new();
            match invalid {
                "checksum" => manifest["article"]["sha256"] = json!("incorrect"),
                "size" => manifest["article"]["byte_count"] = json!(1),
                "format" => manifest["article"]["format"] = json!("pdf"),
                "collision" => {
                    manifest["attachments"] = json!([{"path":"article.html", "source_url":"https://example.com/image.png"}])
                }
                "type" => manifest["capture_type"] = json!("text"),
                "source" => manifest["text"] = json!("selected prose is not a source URL"),
                "utf8" => {
                    content = vec![0xff, 0xfe];
                    manifest["article"]["byte_count"] = json!(2);
                    manifest["article"]["sha256"] = json!(crate::sha256_hex(&content));
                }
                "image-source" => {
                    manifest["attachments"] = json!([{"path":"image.png"}]);
                    extra.push(("image.png", png()));
                }
                _ => unreachable!(),
            }
            let mut entries = vec![("article.html", content.as_slice())];
            entries.extend(extra.iter().map(|(name, bytes)| (*name, bytes.as_slice())));
            let path = archive(&library, &manifest, &entries);
            let report = process_articles(&library, &unavailable_article);
            assert_eq!(report.issues.len(), 1, "{invalid}: {report:?}");
            assert_eq!(report.saved + report.duplicates, 0);
            assert!(path.exists());
            assert!(readings(&library).is_empty());
        }
    }

    #[test]
    fn captured_live_article_survives_a_recognized_source_retrieval_failure() {
        let (_temp, library) = library();
        let html = "<meta property='og:type' content='article'><article><p>Usable captured source content.</p></article>";
        let mut manifest = article_manifest(html);
        manifest["origin"]["url"] = json!("https://x.com/example/status/42");
        let path = archive(&library, &manifest, &[("article.html", html.as_bytes())]);
        let saver = |_: &LibraryRoot, _: UrlSaveRequest| -> Result<SaveOutcome> {
            Err(crate::SaveUrlError::Retrieval("fixture offline".into()).into())
        };
        let report = process_inbox_with_fetchers_at(
            &library,
            &[],
            SystemTime::now() + Duration::from_secs(60),
            None,
            &saver,
            &unavailable_article,
        )
        .unwrap();
        assert_eq!(report.saved, 1, "{report:?}");
        assert!(!path.exists());
        let reading = readings(&library).pop().unwrap();
        assert!(!reading.metadata.lightweight);
        assert!(reading.body.contains("Usable captured source content."));
    }

    #[test]
    fn instagram_requests_import_one_media_and_deduplicate() {
        for video in [false, true] {
            let (_temp, library) = library();
            let manifest = instagram_manifest();
            let resolver = |request: &crate::instagram::InstagramRequest| {
                assert_eq!(request.slide, 2);
                assert!(!request.origin.contains("stkn"));
                let directory = tempfile::tempdir()?;
                let name = if video { "payload.mp4" } else { "payload.jpg" }.to_string();
                fs::write(
                    directory.path().join(&name),
                    if video { movie() } else { png() },
                )?;
                Ok(crate::instagram::DownloadedMedia { directory, name })
            };
            for duplicate in [false, true] {
                let path = archive(&library, &manifest, &[]);
                let report = process_inbox_with_resolver_at(
                    &library,
                    &[],
                    SystemTime::now() + Duration::from_secs(60),
                    Some(&resolver),
                )
                .unwrap();
                assert!(report.issues.is_empty(), "{:?}", report.issues);
                assert_eq!(report.saved, u32::from(!duplicate));
                assert_eq!(report.duplicates, u32::from(duplicate));
                assert!(!path.exists());
                let saved = readings(&library);
                assert_eq!(saved.len(), 1);
                assert_eq!(
                    saved[0].metadata.kind,
                    if video {
                        ReadingKind::Video
                    } else {
                        ReadingKind::Image
                    }
                );
                assert!(!saved[0].metadata.lightweight);
                assert_eq!(saved[0].metadata.saved_at, "2026-09-22T12:00:00.000Z");
                assert_eq!(
                    saved[0].metadata.url,
                    "https://instagram.com/p/DdlVpikk5Gj?img_index=2"
                );
            }
        }
    }

    #[test]
    fn instagram_failures_keep_request_and_never_save_links() {
        let (_temp, library) = library();
        let path = archive(&library, &instagram_manifest(), &[]);
        let offline = process(&library);
        assert_eq!(offline.issues.len(), 1);
        assert!(path.exists() && readings(&library).is_empty());
        let failing = |_: &crate::instagram::InstagramRequest| anyhow::bail!("unavailable");
        let report = process_inbox_with_resolver_at(
            &library,
            &[],
            SystemTime::now() + Duration::from_secs(60),
            Some(&failing),
        )
        .unwrap();
        assert_eq!(report.issues.len(), 1);
        assert!(path.exists() && readings(&library).is_empty());
        let mut invalid = instagram_manifest();
        invalid["text"] = json!("Do not silently save this");
        archive(&library, &invalid, &[]);
        let unexpected =
            |_: &crate::instagram::InstagramRequest| -> Result<crate::instagram::DownloadedMedia> {
                panic!("Invalid request invoked downloader")
            };
        assert_eq!(
            process_inbox_with_resolver_at(
                &library,
                &[],
                SystemTime::now() + Duration::from_secs(60),
                Some(&unexpected)
            )
            .unwrap()
            .issues
            .len(),
            1
        );
        assert!(path.exists() && readings(&library).is_empty());
    }

    #[test]
    #[ignore = "Explicit live Instagram check: requires OIA_TEST_PYTHON and network"]
    fn instagram_live_selected_slide_import() {
        let (_temp, library) = library();
        let path = archive(&library, &instagram_manifest(), &[]);
        let python = std::env::var("OIA_TEST_PYTHON").unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../macos/Resources/instagram-download.py");
        let resolver = |request: &crate::instagram::InstagramRequest| {
            crate::instagram::download(Path::new(&python), &script, request)
        };
        let report = process_inbox_with_resolver_at(
            &library,
            &[],
            SystemTime::now() + Duration::from_secs(60),
            Some(&resolver),
        )
        .unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(report.saved, 1);
        assert!(!path.exists());
        let saved = readings(&library);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].metadata.kind, ReadingKind::Image);
        assert_eq!(
            saved[0].metadata.url,
            "https://instagram.com/p/DdlVpikk5Gj?img_index=2"
        );
        assert!(
            saved[0]
                .metadata
                .media_url
                .as_ref()
                .unwrap()
                .contains("01aedfae8b4e7ab5e209fcc8c97ee123f4c20658459d5c82854d5c6245b89e5e"),
            "Live media differs from the independently verified slide 2 fixture"
        );
    }

    #[test]
    fn creates_missing_inbox_without_reading_other_library_files() {
        let temp = TempDir::new().unwrap();
        let library = LibraryRoot::new(temp.path()).unwrap();
        fs::write(temp.path().join("leave.txt"), "leave me").unwrap();
        assert_eq!(process(&library), InboxReport::default());
        assert!(library.inbox_dir().is_dir());
        assert!(temp.path().join("leave.txt").exists());
    }

    #[test]
    fn saves_image_text_and_link_then_removes_only_successful_inputs() {
        let (_temp, library) = library();
        fs::write(library.inbox_dir().join("photo.PNG"), png()).unwrap();
        fs::write(library.inbox_dir().join("quote.txt"), "Keep this\nline too").unwrap();
        fs::write(
            library.inbox_dir().join("link.txt"),
            "https://example.com/story?utm_source=phone",
        )
        .unwrap();
        fs::write(library.inbox_dir().join("unsupported.pdf"), "not supported").unwrap();
        let report = process(&library);
        assert_eq!(report.saved, 3, "{report:?}");
        assert_eq!(report.issues.len(), 1);
        assert_eq!(fs::read_dir(library.inbox_dir()).unwrap().count(), 1);
        let readings = readings(&library);
        assert!(readings
            .iter()
            .any(|reading| reading.metadata.kind == ReadingKind::Image));
        assert!(readings
            .iter()
            .any(|reading| reading.metadata.kind == ReadingKind::Quote));
        assert!(readings.iter().any(|reading| reading.metadata.lightweight
            && reading.metadata.url == "https://example.com/story"));
    }

    #[test]
    fn defers_young_and_cloud_excluded_files_without_blocking_ready_files() {
        let (_temp, library) = library();
        fs::write(library.inbox_dir().join("ready.txt"), "ready").unwrap();
        fs::write(
            library.inbox_dir().join("cloud.txt"),
            "outdated local bytes",
        )
        .unwrap();
        let young = process_inbox(&library, &[]).unwrap();
        assert_eq!(young.pending, 2);
        assert_eq!(young.saved, 0);
        let report = process_inbox_at(
            &library,
            &["cloud.txt".to_string(), "missing.txt".to_string()],
            SystemTime::now() + Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(report.pending, 1);
        assert_eq!(report.saved, 1);
        assert!(library.inbox_dir().join("cloud.txt").exists());
    }

    #[test]
    fn ignores_symlinks_hidden_files_and_arbitrary_directories() {
        let (_temp, library) = library();
        fs::write(library.path().join("outside.txt"), "outside").unwrap();
        symlink(
            library.path().join("outside.txt"),
            library.inbox_dir().join("link.txt"),
        )
        .unwrap();
        fs::write(library.inbox_dir().join(".partial.txt"), "partial").unwrap();
        fs::create_dir(library.inbox_dir().join("folder")).unwrap();
        fs::write(library.inbox_dir().join("folder/inside.txt"), "inside").unwrap();
        assert_eq!(process(&library), InboxReport::default());
        assert!(library.path().join("outside.txt").exists());
    }

    #[test]
    fn refuses_symlinked_inbox() {
        let temp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let library = LibraryRoot::new(temp.path()).unwrap();
        symlink(outside.path(), library.inbox_dir()).unwrap();
        assert!(process_inbox(&library, &[]).is_err());
    }

    #[test]
    fn consumes_exact_duplicate_only_after_verifying_local_image() {
        let (_temp, library) = library();
        let source = library.inbox_dir().join("photo.png");
        fs::write(&source, png()).unwrap();
        assert_eq!(process(&library).saved, 1);
        fs::write(&source, png()).unwrap();
        let report = process(&library);
        assert_eq!(report.duplicates, 1, "{report:?}");
        assert!(!source.exists());
        assert_eq!(readings(&library).len(), 1);
    }

    #[test]
    fn retains_image_when_existing_duplicate_is_missing_or_corrupt() {
        let (_temp, library) = library();
        let source = library.inbox_dir().join("photo.png");
        fs::write(&source, png()).unwrap();
        assert_eq!(process(&library).saved, 1);
        let reading = readings(&library).pop().unwrap();
        let asset = library
            .reading_dir(&reading.metadata.id)
            .join(crate::first_local_image_asset(&reading.body).unwrap());
        fs::remove_file(&asset).unwrap();
        fs::write(&source, png()).unwrap();
        assert_eq!(process(&library).issues.len(), 1);
        assert!(source.exists());
        fs::write(&asset, "wrong bytes").unwrap();
        assert_eq!(process(&library).issues.len(), 1);
        assert!(source.exists());
    }

    #[test]
    fn retains_text_when_duplicate_quote_body_is_missing_or_edited() {
        for body in ["", "> A deliberate edit that must also be preserved"] {
            let (_temp, library) = library();
            let source = library.inbox_dir().join("note.txt");
            let text = "The original thought\n> with a literal quote marker";
            fs::write(&source, text).unwrap();
            assert_eq!(process(&library).saved, 1);
            let mut reading = readings(&library).pop().unwrap();
            reading.body = body.to_string();
            let changed = crate::render_reading(&reading).unwrap();
            fs::write(library.article_path(&reading.metadata.id), &changed).unwrap();

            fs::write(&source, text).unwrap();
            let report = process(&library);
            assert_eq!(report.issues.len(), 1, "{report:?}");
            assert_eq!(report.saved + report.duplicates, 0);
            assert_eq!(fs::read_to_string(source).unwrap(), text);
            assert_eq!(
                fs::read_to_string(library.article_path(&reading.metadata.id)).unwrap(),
                changed
            );
        }
    }

    #[test]
    fn retains_sourced_quote_archive_when_body_or_source_has_changed() {
        for change_source in [false, true] {
            let (_temp, library) = library();
            let mut manifest = manifest();
            manifest["origin"] = json!({"url":"https://example.com/source"});
            manifest["text"] = json!("“Selected words from the original page”");
            archive(&library, &manifest, &[]);
            assert_eq!(process(&library).saved, 1);
            let mut reading = readings(&library).pop().unwrap();
            if change_source {
                reading.metadata.url = "https://example.com/different-page".to_string();
            } else {
                reading.body.clear();
            }
            let changed = crate::render_reading(&reading).unwrap();
            fs::write(library.article_path(&reading.metadata.id), &changed).unwrap();
            let source = archive(&library, &manifest, &[]);

            assert_eq!(process(&library).issues.len(), 1);
            assert!(source.exists());
            assert_eq!(
                fs::read_to_string(library.article_path(&reading.metadata.id)).unwrap(),
                changed
            );
        }
    }

    #[test]
    fn quote_verification_preserves_literal_markers_and_whitespace_deduplication() {
        for sourced in [false, true] {
            let (_temp, library) = library();
            let mut manifest = manifest();
            if sourced {
                manifest["origin"] = json!({"url":"https://example.com/source"});
            }
            manifest["text"] = json!(if sourced {
                "“First line\n\n> literal marker\n>> two markers”"
            } else {
                "First line\n\n> literal marker\n>> two markers"
            });
            archive(&library, &manifest, &[]);
            assert_eq!(process(&library).saved, 1);
            let mut reading = readings(&library).pop().unwrap();
            reading.metadata.title = "My edited title".to_string();
            let changed = crate::render_reading(&reading).unwrap();
            fs::write(library.article_path(&reading.metadata.id), &changed).unwrap();
            manifest["text"] = json!(if sourced {
                "“First line  > literal marker\t>> two markers”"
            } else {
                "First line  > literal marker\t>> two markers"
            });
            let source = archive(&library, &manifest, &[]);

            let report = process(&library);
            assert_eq!(report.duplicates, 1, "{report:?}");
            assert!(!source.exists());
            assert_eq!(
                fs::read_to_string(library.article_path(&reading.metadata.id)).unwrap(),
                changed
            );
        }
    }

    #[test]
    fn duplicate_link_preserves_edited_title_and_article_body() {
        for lightweight in [false, true] {
            let (_temp, library) = library();
            let mut manifest = manifest();
            manifest["origin"] = json!({"url":"https://example.com/source", "title":"Page title"});
            archive(&library, &manifest, &[]);
            assert_eq!(process(&library).saved, 1);
            let mut reading = readings(&library).pop().unwrap();
            reading.metadata.title = "My edited title".to_string();
            reading.metadata.lightweight = lightweight;
            reading.body = "My existing content".to_string();
            let changed = crate::render_reading(&reading).unwrap();
            fs::write(library.article_path(&reading.metadata.id), &changed).unwrap();
            let source = archive(&library, &manifest, &[]);

            let report = process(&library);
            assert_eq!(report.duplicates, 1, "{report:?}");
            assert!(!source.exists());
            assert_eq!(
                fs::read_to_string(library.article_path(&reading.metadata.id)).unwrap(),
                changed
            );
        }
    }

    #[test]
    fn incomplete_or_mislabeled_images_are_retained() {
        let (_temp, library) = library();
        let mut partial = png();
        partial.truncate(partial.len() - 12);
        fs::write(library.inbox_dir().join("partial.png"), partial).unwrap();
        fs::write(library.inbox_dir().join("fake.jpg"), "not a JPEG").unwrap();
        let report = process(&library);
        assert_eq!(report.issues.len(), 2);
        assert_eq!(report.saved, 0);
    }

    #[test]
    fn snapshot_will_not_delete_replaced_or_edited_source() {
        let (_temp, library) = library();
        let directory = open_regular_directory(&library.inbox_dir()).unwrap();
        let path = library.inbox_dir().join("note.txt");
        fs::write(&path, "first content").unwrap();
        let snapshot = Snapshot::read(&path, &fs::metadata(&path).unwrap())
            .unwrap()
            .unwrap();
        fs::write(&path, "other content").unwrap();
        assert!(!snapshot
            .remove_if_unchanged(&directory, "note.txt")
            .unwrap());
        fs::remove_file(&path).unwrap();
        fs::write(&path, "first content").unwrap();
        assert!(!snapshot
            .remove_if_unchanged(&directory, "note.txt")
            .unwrap());
        assert!(path.exists());
    }

    #[test]
    fn cleanup_keeps_replacement_arriving_between_preflight_and_claim() {
        let (_temp, library) = library();
        let directory = open_regular_directory(&library.inbox_dir()).unwrap();
        let path = library.inbox_dir().join("note.txt");
        fs::write(&path, "first content").unwrap();
        let snapshot = Snapshot::read(&path, &fs::metadata(&path).unwrap())
            .unwrap()
            .unwrap();
        let replacement = library.path().join("replacement.txt");
        fs::write(&replacement, "the new original must survive").unwrap();

        let claim = snapshot
            .claim_source(&directory, "note.txt", || {
                fs::rename(&replacement, &path).unwrap();
            })
            .unwrap();
        let SourceClaim::Claimed(name) = claim else {
            panic!("expected the replacement to be claimed")
        };
        assert!(name.starts_with("recovered-"));
        assert!(name.ends_with("-note.txt"));
        assert!(!snapshot
            .remove_claimed_if_unchanged(&directory, &name)
            .unwrap());
        assert_eq!(
            fs::read_to_string(library.inbox_dir().join(&name)).unwrap(),
            "the new original must survive"
        );

        assert_eq!(process(&library).saved, 1);
        assert!(readings(&library)[0]
            .body
            .contains("the new original must survive"));
    }

    #[test]
    fn crash_after_claim_leaves_visible_archive_for_deduplicated_retry() {
        let (_temp, library) = library();
        let directory = open_regular_directory(&library.inbox_dir()).unwrap();
        let mut manifest = manifest();
        manifest["text"] = json!("Already saved before the interrupted cleanup");
        let path = archive(&library, &manifest, &[]);
        let mut snapshot = Snapshot::read(&path, &fs::metadata(&path).unwrap())
            .unwrap()
            .unwrap();
        import_capture(
            &library,
            &mut snapshot.file,
            &mut Vec::new(),
            None,
            &save_offline_url,
            &unavailable_article,
        )
        .unwrap();
        let SourceClaim::Claimed(name) = snapshot
            .claim_source(
                &directory,
                path.file_name().unwrap().to_str().unwrap(),
                || {},
            )
            .unwrap()
        else {
            panic!("expected an interrupted cleanup claim")
        };
        drop(snapshot); // Simulate losing in-memory state after the durable rename.

        assert!(name.starts_with("recovered-"));
        assert!(name.ends_with(".cuttingscapture.zip"));
        assert!(library.inbox_dir().join(name).is_file());
        let report = process(&library);
        assert_eq!(report.duplicates, 1, "{report:?}");
        assert!(report.issues.is_empty());
        assert_eq!(readings(&library).len(), 1);
        assert_eq!(fs::read_dir(library.inbox_dir()).unwrap().count(), 0);
    }

    #[test]
    fn cleanup_does_not_remove_new_arrival_at_original_name_after_claim() {
        let (_temp, library) = library();
        let directory = open_regular_directory(&library.inbox_dir()).unwrap();
        let path = library.inbox_dir().join("note.txt");
        fs::write(&path, "saved content").unwrap();
        let snapshot = Snapshot::read(&path, &fs::metadata(&path).unwrap())
            .unwrap()
            .unwrap();
        let SourceClaim::Claimed(name) = snapshot
            .claim_source(&directory, "note.txt", || {})
            .unwrap()
        else {
            panic!("expected a cleanup claim")
        };
        fs::write(&path, "next arrival").unwrap();

        assert!(snapshot
            .remove_claimed_if_unchanged(&directory, &name)
            .unwrap());
        assert_eq!(fs::read_to_string(path).unwrap(), "next arrival");
    }

    #[test]
    fn cleanup_stays_with_pinned_directory_when_inbox_path_is_replaced() {
        let (_temp, library) = library();
        let directory = open_regular_directory(&library.inbox_dir()).unwrap();
        let path = library.inbox_dir().join("note.txt");
        fs::write(&path, "saved content").unwrap();
        let snapshot = Snapshot::read(&path, &fs::metadata(&path).unwrap())
            .unwrap()
            .unwrap();
        let old_inbox = library.path().join("previous-inbox");
        fs::rename(library.inbox_dir(), &old_inbox).unwrap();
        fs::create_dir(library.inbox_dir()).unwrap();
        fs::write(&path, "new directory content").unwrap();

        assert!(snapshot
            .remove_if_unchanged(&directory, "note.txt")
            .unwrap());
        assert_eq!(fs::read_to_string(path).unwrap(), "new directory content");
        assert_eq!(fs::read_dir(old_inbox).unwrap().count(), 0);
    }

    #[test]
    fn recovery_names_preserve_supported_extensions_with_bounded_utf8_names() {
        for suffix in [".txt", ".cuttingscapture.zip"] {
            let original = format!("{}{suffix}", "é".repeat(110));
            let recovered = recovery_name(&original);
            assert!(recovered.len() <= 255);
            assert!(recovered.starts_with("recovered-"));
            assert!(recovered.ends_with(suffix));
        }
    }

    #[test]
    fn imports_windows_and_xml_and_binary_mac_link_files() {
        let (_temp, library) = library();
        fs::write(
            library.inbox_dir().join("windows.url"),
            "[InternetShortcut]\r\nURL=https://example.com/windows\r\n",
        )
        .unwrap();
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "URL".into(),
            plist::Value::String("https://example.com/mac".into()),
        );
        let value = plist::Value::Dictionary(dict);
        value
            .to_file_xml(library.inbox_dir().join("mac.webloc"))
            .unwrap();
        value
            .to_file_binary(library.inbox_dir().join("same.webloc"))
            .unwrap();
        let report = process(&library);
        assert_eq!(report.saved, 2, "{report:?}");
        assert_eq!(report.duplicates, 1);
    }

    #[test]
    fn safari_link_capture_keeps_origin_title_canonical_site_and_capture_date() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["origin"] = json!({"url":"https://example.com/source?utm_source=share", "title":"Page title", "canonical_url":"https://example.com/canonical", "site_name":"Example publication"});
        let path = archive(&library, &manifest, &[]);
        let report = process(&library);
        assert_eq!(report.saved, 1, "{report:?}");
        assert!(!path.exists());
        let reading = readings(&library).pop().unwrap();
        assert_eq!(reading.metadata.url, "https://example.com/source");
        assert_eq!(
            reading.metadata.canonical_url,
            "https://example.com/canonical"
        );
        assert_eq!(reading.metadata.title, "Page title");
        assert_eq!(
            reading.metadata.site.as_deref(),
            Some("Example publication")
        );
        assert_eq!(reading.metadata.saved_at, "2026-09-10T11:34:56.000Z");
        assert!(reading.metadata.lightweight);
    }

    #[test]
    fn legacy_safari_selection_becomes_highlight_on_the_source_article() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["origin"] = json!({"url":"https://example.com/source", "title":"Article"});
        manifest["text"] = json!("A selected thought\nwith another line.");
        archive(&library, &manifest, &[]);
        assert_eq!(process(&library).saved, 1);
        let reading = readings(&library).pop().unwrap();
        assert_eq!(reading.metadata.kind, ReadingKind::Article);
        assert_eq!(reading.metadata.url, "https://example.com/source");
        assert!(reading.metadata.lightweight);
        let highlights = crate::list_highlights(&library, &reading.metadata.id).unwrap();
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].text, "A selected thought\nwith another line.");
        assert_eq!(
            reading.metadata.featured_highlight_id.as_deref(),
            Some(highlights[0].id.as_str())
        );
    }

    #[test]
    fn url_and_text_url_captures_use_one_saver_and_preserve_categories() {
        for version in [1, 3] {
            for text_input in [false, true] {
                let (_temp, library) = library();
                let mut manifest = manifest();
                manifest["version"] = json!(version);
                let url = "https://www.goodreads.com/book/show/12345-title";
                if text_input {
                    manifest["text"] = json!(url);
                    manifest["capture_type"] = json!("text");
                } else {
                    manifest["origin"] = json!({"url":url, "title":"Shared book"});
                    manifest["capture_type"] = json!("url");
                }
                let path = archive(&library, &manifest, &[]);
                let requests = std::cell::RefCell::new(Vec::new());
                let saver = |library: &LibraryRoot, request: UrlSaveRequest| {
                    requests.borrow_mut().push(request.clone());
                    save_offline_url(library, request)
                };
                let report = process_inbox_with_savers_at(
                    &library,
                    &[],
                    SystemTime::now() + Duration::from_secs(60),
                    None,
                    &saver,
                )
                .unwrap();
                assert_eq!(report.saved, 1, "{report:?}");
                assert!(!path.exists());
                assert_eq!(requests.borrow().len(), 1);
                let reading = readings(&library).pop().unwrap();
                assert_eq!(reading.metadata.link_category.as_deref(), Some("book"));
                assert_eq!(reading.metadata.url, crate::normalize_url(url).unwrap());
                if !text_input {
                    assert_eq!(reading.metadata.title, "Shared book");
                }
            }
        }
    }

    #[test]
    fn typed_selection_preserves_article_and_features_stable_passage_ids() {
        let (_temp, library) = library();
        let url = "https://example.com/source";
        let saved = save_offline_url(&library, UrlSaveRequest::new(url)).unwrap();
        let path = library.article_path(&saved.id);
        let mut reading = parse_reading(&fs::read_to_string(&path).unwrap()).unwrap();
        reading.body = "Original captured article body.\n\nKeep this text.".into();
        reading.metadata.lightweight = false;
        reading.metadata.title = "My edited title".into();
        reading.metadata.tags = vec!["kept".into()];
        reading.metadata.favorite = true;
        reading.metadata.link_category = Some("movie".into());
        fs::write(&path, crate::render_reading(&reading).unwrap()).unwrap();
        let reading = parse_reading(&fs::read_to_string(&path).unwrap()).unwrap();
        let before = reading.metadata.clone();
        let mut manifest = manifest();
        manifest["version"] = json!(3);
        manifest["capture_type"] = json!("selection");
        manifest["origin"] = json!({"url":url, "title":"Replacement title"});
        for text in ["First passage.", "Second passage.", "First passage."] {
            manifest["text"] = json!(text);
            let archive = archive(&library, &manifest, &[]);
            let report = process(&library);
            assert_eq!(report.saved, 1, "{report:?}");
            assert!(!archive.exists());
        }
        assert_eq!(readings(&library).len(), 1);
        let updated = parse_reading(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(updated.body, reading.body);
        assert_eq!(updated.metadata.title, before.title);
        assert_eq!(updated.metadata.tags, before.tags);
        assert_eq!(updated.metadata.favorite, before.favorite);
        assert_eq!(updated.metadata.saved_at, before.saved_at);
        assert_eq!(updated.metadata.link_category, before.link_category);
        assert_eq!(updated.metadata.source_hash, before.source_hash);
        let highlights = crate::list_highlights(&library, &saved.id).unwrap();
        assert_eq!(highlights.len(), 2);
        assert_eq!(
            updated.metadata.featured_highlight_id.as_deref(),
            Some(highlights[0].id.as_str())
        );
    }

    #[test]
    fn selection_type_keeps_selected_url_as_a_highlight() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["version"] = json!(3);
        manifest["capture_type"] = json!("selection");
        manifest["origin"] = json!({"url":"https://example.com/source"});
        manifest["text"] = json!("https://example.com/source");
        archive(&library, &manifest, &[]);
        assert_eq!(process(&library).saved, 1);
        let reading = readings(&library).pop().unwrap();
        assert_eq!(
            crate::list_highlights(&library, &reading.metadata.id).unwrap()[0].text,
            "https://example.com/source"
        );
    }

    #[test]
    fn highlight_uses_resolved_alias_identity_without_an_extra_article() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["version"] = json!(3);
        manifest["capture_type"] = json!("selection");
        manifest["origin"] = json!({"url":"https://example.com/alias"});
        manifest["text"] = json!("Selected passage");
        let path = archive(&library, &manifest, &[]);
        let saver = |library: &LibraryRoot, mut request: UrlSaveRequest| {
            assert_eq!(request.url, "https://example.com/alias");
            request.url = "https://example.com/canonical".into();
            save_offline_url(library, request)
        };
        let report = process_inbox_with_savers_at(
            &library,
            &[],
            SystemTime::now() + Duration::from_secs(60),
            None,
            &saver,
        )
        .unwrap();
        assert_eq!(report.saved, 1, "{report:?}");
        assert!(!path.exists());
        let saved = readings(&library);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].metadata.url, "https://example.com/canonical");
        assert_eq!(
            crate::list_highlights(&library, &saved[0].metadata.id).unwrap()[0].text,
            "Selected passage"
        );
    }

    #[test]
    fn highlight_verification_requires_matching_source_text_and_featured_id() {
        let (_temp, library) = library();
        let url = "https://example.com/source";
        let outcome = import_text_capture(
            &library,
            "Selected passage",
            &Origin {
                url: Some(url.into()),
                ..Origin::default()
            },
            None,
            Some(CaptureType::Selection),
            &save_offline_url,
        )
        .unwrap();
        assert!(verify_saved(
            &library,
            &outcome,
            SavedContent::Highlight {
                text: "Selected passage",
                url,
            }
        )
        .is_ok());
        for (text, source) in [
            ("Other passage", url),
            ("Selected passage", "https://example.com/other"),
        ] {
            assert!(verify_saved(
                &library,
                &outcome,
                SavedContent::Highlight { text, url: source }
            )
            .is_err());
        }
        let path = library.article_path(&outcome.id);
        let mut reading = parse_reading(&fs::read_to_string(&path).unwrap()).unwrap();
        reading.metadata.featured_highlight_id = Some("missing-highlight".into());
        fs::write(&path, crate::render_reading(&reading).unwrap()).unwrap();
        assert!(verify_saved(
            &library,
            &outcome,
            SavedContent::Highlight {
                text: "Selected passage",
                url,
            }
        )
        .is_err());
    }

    #[test]
    fn typed_quoted_selection_saves_quote_without_url_retrieval() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["version"] = json!(3);
        manifest["capture_type"] = json!("selection");
        manifest["origin"] = json!({"url":"https://example.com/source"});
        manifest["text"] = json!("“A quoted thought\non two lines.”");
        let path = archive(&library, &manifest, &[]);
        let saver = |_: &LibraryRoot, _: UrlSaveRequest| -> Result<SaveOutcome> {
            panic!("A quoted selection must remain offline")
        };
        let report = process_inbox_with_savers_at(
            &library,
            &[],
            SystemTime::now() + Duration::from_secs(60),
            None,
            &saver,
        )
        .unwrap();
        assert_eq!(report.saved, 1, "{report:?}");
        assert!(!path.exists());
        let reading = readings(&library).pop().unwrap();
        assert_eq!(reading.metadata.kind, ReadingKind::Quote);
        assert_eq!(
            reading.metadata.id,
            crate::quote_id(
                "https://example.com/source",
                "“A quoted thought\non two lines.”"
            )
            .unwrap()
        );
        assert!(reading
            .body
            .contains("> “A quoted thought\n> on two lines.”"));
    }

    #[test]
    fn typed_capture_mismatches_retain_archive_without_saving_payloads() {
        let cases = [
            json!({"text":"hello"}),
            json!({"capture_type":"book", "text":"hello"}),
            json!({"capture_type":"url", "origin":{"url":"https://example.com/source"}, "text":"selected prose"}),
            json!({"capture_type":"url", "origin":{"url":"https://example.com/source"}, "text":"https://example.com/different"}),
            json!({"capture_type":"url", "origin":{"url":"https://example.com/source"}, "attachments":[{"path":"image.png"}]}),
            json!({"capture_type":"selection", "text":"no source"}),
            json!({"capture_type":"selection", "origin":{"url":"https://example.com/source"}, "text":"a".repeat(crate::MAX_SELECTION_TEXT_BYTES + 1)}),
            json!({"capture_type":"text", "origin":{"url":"https://example.com/source"}, "text":"unexpected source"}),
            json!({"capture_type":"media", "text":"other payload", "attachments":[{"path":"image.png"}]}),
        ];
        for case in cases {
            let (_temp, library) = library();
            let mut manifest = manifest();
            manifest["version"] = json!(3);
            for (key, value) in case.as_object().unwrap() {
                manifest[key] = value.clone();
            }
            let bytes = png();
            let payloads = if manifest["attachments"].is_array() {
                vec![("image.png", bytes.as_slice())]
            } else {
                vec![]
            };
            let path = archive(&library, &manifest, &payloads);
            let report = process(&library);
            assert_eq!(report.issues.len(), 1, "{report:?}");
            assert_eq!(report.saved + report.duplicates, 0);
            assert!(path.exists());
            assert!(readings(&library).is_empty());
        }
    }

    #[test]
    fn typed_media_still_accepts_supported_text_file_attachments() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["version"] = json!(3);
        manifest["capture_type"] = json!("media");
        manifest["attachments"] = json!([{"path":"note.txt"}]);
        let path = archive(&library, &manifest, &[("note.txt", b"A shared text file")]);
        assert_eq!(process(&library).saved, 1);
        assert!(!path.exists());
        assert_eq!(readings(&library)[0].metadata.kind, ReadingKind::Quote);
    }

    #[test]
    fn url_resolution_and_highlight_sidecar_failures_retain_typed_archives() {
        for fail_resolution in [false, true] {
            let (_temp, library) = library();
            let url = "https://example.com/source";
            let mut manifest = manifest();
            manifest["version"] = json!(3);
            manifest["capture_type"] = json!("selection");
            manifest["origin"] = json!({"url":url});
            manifest["text"] = json!("Selected passage");
            if !fail_resolution {
                let saved = save_offline_url(&library, UrlSaveRequest::new(url)).unwrap();
                fs::create_dir(library.highlights_path(&saved.id)).unwrap();
            }
            let path = archive(&library, &manifest, &[]);
            let saver = |library: &LibraryRoot, request: UrlSaveRequest| {
                if fail_resolution {
                    bail!("fixture source retrieval failed")
                }
                save_offline_url(library, request)
            };
            let report = process_inbox_with_savers_at(
                &library,
                &[],
                SystemTime::now() + Duration::from_secs(60),
                None,
                &saver,
            )
            .unwrap();
            assert_eq!(report.issues.len(), 1, "{report:?}");
            assert!(path.exists());
            assert_eq!(report.saved + report.duplicates, 0);
        }
    }

    #[test]
    fn media_capture_keeps_origin_and_verifies_declared_size_and_hash() {
        let (_temp, library) = library();
        let bytes = png();
        let mut manifest = manifest();
        manifest["origin"] = json!({"url":"https://example.com/source", "canonical_url":"https://example.com/canonical", "site_name":"Example", "title":"Source image"});
        manifest["attachments"] = json!([{"path":"payload/image.png", "byte_count":bytes.len(), "sha256":crate::sha256_hex(&bytes)}]);
        archive(&library, &manifest, &[("payload/image.png", &bytes)]);
        assert_eq!(process(&library).saved, 1);
        let reading = readings(&library).pop().unwrap();
        assert_eq!(reading.metadata.url, "https://example.com/source");
        assert_eq!(
            reading.metadata.canonical_url,
            "https://example.com/canonical"
        );
        assert_eq!(reading.metadata.site.as_deref(), Some("Example"));
        manifest["attachments"][0]["sha256"] = json!("incorrect");
        let path = archive(&library, &manifest, &[("payload/image.png", &bytes)]);
        assert_eq!(process(&library).issues.len(), 1);
        assert!(path.exists());
    }

    #[test]
    fn invalid_version_missing_payload_and_extra_files_are_retained() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["version"] = json!(2);
        manifest["text"] = json!("hello");
        let path = archive(&library, &manifest, &[]);
        assert_eq!(process(&library).issues.len(), 1);
        assert!(path.exists());
        manifest["version"] = json!(1);
        manifest["attachments"] = json!([{"path":"missing.png"}]);
        archive(&library, &manifest, &[]);
        assert_eq!(process(&library).issues.len(), 1);
        manifest["attachments"] = json!([]);
        archive(
            &library,
            &manifest,
            &[("unlisted.txt", b"do not lose this")],
        );
        assert_eq!(process(&library).issues.len(), 1);
        assert_eq!(readings(&library).len(), 0);
    }

    #[test]
    fn unsafe_archive_paths_and_truncated_archives_are_retained() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["text"] = json!("hello");
        let path = archive(&library, &manifest, &[("../escape.txt", b"bad path")]);
        assert_eq!(process(&library).issues.len(), 1);
        assert!(path.exists());
        assert!(!library.path().join("escape.txt").exists());
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() - 15]).unwrap();
        assert_eq!(process(&library).issues.len(), 1);
        assert!(path.exists());
    }

    #[test]
    fn partial_capture_success_is_counted_and_retry_deduplicates() {
        let (_temp, library) = library();
        let bytes = png();
        let mut manifest = manifest();
        manifest["attachments"] = json!([{"path":"image.png"}, {"path":"unsupported.pdf"}]);
        let path = archive(
            &library,
            &manifest,
            &[("image.png", &bytes), ("unsupported.pdf", b"unsupported")],
        );
        let report = process(&library);
        assert_eq!(report.saved, 1, "{report:?}");
        assert_eq!(report.issues.len(), 1);
        assert!(path.exists());
        let retry = process(&library);
        assert_eq!(retry.saved, 0);
        assert_eq!(retry.duplicates, 1);
        assert_eq!(readings(&library).len(), 1);
    }

    #[test]
    fn archive_symlink_entries_are_rejected() {
        let (_temp, library) = library();
        let path = library.inbox_dir().join("symlink.cuttingscapture.zip");
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        zip.add_symlink("link", "/etc/passwd", SimpleFileOptions::default())
            .unwrap();
        zip.finish().unwrap();
        assert_eq!(process(&library).issues.len(), 1);
        assert!(path.exists());
    }

    #[test]
    fn unreferenced_directory_entries_cannot_hide_payload_bytes() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["text"] = json!("Keep both this text and the unlisted bytes");
        let path = archive(&library, &manifest, &[("unlisted/", b"do not discard")]);
        let original = fs::read(&path).unwrap();

        let report = process(&library);
        assert_eq!(report.issues.len(), 1, "{report:?}");
        assert_eq!(report.saved, 0);
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn directory_entry_types_must_match_their_names() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["text"] = json!("Do not consume a malformed archive");
        // start_file marks this as a regular file, even though its name has a
        // trailing slash. Empty bytes alone must not make it a valid directory.
        let path = archive(&library, &manifest, &[("not-a-directory/", b"")]);
        assert_eq!(process(&library).issues.len(), 1);
        assert!(path.exists());
    }

    #[test]
    fn ordinary_empty_archive_directories_are_accepted() {
        let (_temp, library) = library();
        let mut manifest = manifest();
        manifest["text"] = json!("A complete capture with an empty payload directory");
        let path = library.inbox_dir().join("directories.cuttingscapture.zip");
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        zip.add_directory("payload/", SimpleFileOptions::default())
            .unwrap();
        zip.start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        zip.finish().unwrap();

        let report = process(&library);
        assert_eq!(report.saved, 1, "{report:?}");
        assert!(report.issues.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn media_verification_rejects_changed_origin_with_the_original_id() {
        for (name, bytes) in [("image.png", png()), ("video.mov", movie())] {
            let (_temp, library) = library();
            let source = library.inbox_dir().join(name);
            fs::write(&source, &bytes).unwrap();
            assert_eq!(process(&library).saved, 1);
            let mut reading = readings(&library).pop().unwrap();
            reading.metadata.url = "https://example.com/not-the-original-source".to_string();
            fs::write(
                library.article_path(&reading.metadata.id),
                crate::render_reading(&reading).unwrap(),
            )
            .unwrap();
            // Exercise verification directly: some shared save paths repair a
            // mismatched metadata identity before they return their outcome.
            let outcome = SaveOutcome {
                disposition: SaveDisposition::Duplicate,
                id: reading.metadata.id,
                path: String::new(),
            };
            let error = verify_saved(
                &library,
                &outcome,
                SavedContent::MediaHash(&crate::sha256_hex(&bytes)),
            )
            .unwrap_err();
            assert!(error.to_string().contains("unexpected source"));
        }
    }

    #[test]
    fn raw_video_and_sourced_video_keep_local_assets_and_deduplicate() {
        let (_temp, library) = library();
        let bytes = movie();
        let source = library.inbox_dir().join("movie.mp4");
        fs::write(&source, &bytes).unwrap();
        let report = process(&library);
        assert_eq!(report.saved, 1, "{report:?}");
        assert!(!source.exists());
        fs::write(&source, &bytes).unwrap();
        assert_eq!(process(&library).duplicates, 1);
        let mut manifest = manifest();
        manifest["origin"] = json!({"url":"https://example.com/movie", "canonical_url":"https://example.com/original", "site_name":"Example", "title":"A film"});
        manifest["attachments"] = json!([{"path":"payload.mov"}]);
        archive(&library, &manifest, &[("payload.mov", &bytes)]);
        let report = process(&library);
        assert_eq!(report.saved, 1, "{report:?}");
        let readings = readings(&library);
        assert_eq!(readings.len(), 2);
        let sourced = readings
            .iter()
            .find(|reading| reading.metadata.url == "https://example.com/movie")
            .unwrap();
        assert_eq!(sourced.metadata.kind, ReadingKind::Video);
        assert_eq!(
            sourced.metadata.canonical_url,
            "https://example.com/original"
        );
        assert_eq!(sourced.metadata.site.as_deref(), Some("Example"));
        let asset = sourced
            .metadata
            .media_url
            .as_deref()
            .unwrap()
            .strip_prefix("cuttings-asset:")
            .unwrap();
        assert_eq!(
            fs::read(library.reading_dir(&sourced.metadata.id).join(asset)).unwrap(),
            bytes
        );
    }

    #[test]
    fn rejects_truncated_video_and_oversized_sparse_input() {
        let (_temp, library) = library();
        let mut bytes = movie();
        bytes.pop();
        fs::write(library.inbox_dir().join("partial.mp4"), bytes).unwrap();
        File::create(library.inbox_dir().join("huge.mov"))
            .unwrap()
            .set_len(MAX_CAPTURE_BYTES + 1)
            .unwrap();
        let report = process(&library);
        assert_eq!(report.issues.len(), 2);
        assert_eq!(report.saved, 0);
    }

    #[test]
    fn crc_mismatch_retains_capture_without_importing_payload() {
        let (_temp, library) = library();
        let bytes = png();
        let mut manifest = manifest();
        manifest["attachments"] = json!([{"path":"payload.png"}]);
        let path = archive(&library, &manifest, &[("payload.png", &bytes)]);
        let mut archive_bytes = fs::read(&path).unwrap();
        let position = archive_bytes
            .windows(bytes.len())
            .position(|window| window == bytes)
            .unwrap();
        archive_bytes[position + 20] ^= 1;
        fs::write(&path, archive_bytes).unwrap();
        let report = process(&library);
        assert_eq!(report.issues.len(), 1, "{report:?}");
        assert_eq!(report.saved, 0);
        assert!(path.exists());
    }

    #[test]
    fn symlinked_duplicate_asset_is_not_consumed() {
        let (_temp, library) = library();
        let source = library.inbox_dir().join("photo.png");
        fs::write(&source, png()).unwrap();
        assert_eq!(process(&library).saved, 1);
        let reading = readings(&library).pop().unwrap();
        let asset = library
            .reading_dir(&reading.metadata.id)
            .join(crate::first_local_image_asset(&reading.body).unwrap());
        fs::remove_file(&asset).unwrap();
        let outside = library.path().join("outside.png");
        fs::write(&outside, png()).unwrap();
        symlink(&outside, &asset).unwrap();
        fs::write(&source, png()).unwrap();
        assert_eq!(process(&library).issues.len(), 1);
        assert!(source.exists());
        assert_eq!(fs::read(outside).unwrap(), png());
    }
}
