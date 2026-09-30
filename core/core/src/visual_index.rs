// SPDX-License-Identifier: MIT

//! Disposable, device-local visual analysis derived from reading assets.
//!
//! The platform client performs image decoding and Vision classification. This
//! module owns every durable semantic decision: safe asset discovery, byte
//! identity, confidence policy, identifier normalization, colour families, and
//! the analyzer-versioned cache.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read as _, Seek as _, SeekFrom, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use rustix::{
    fs::{AtFlags, Mode, OFlags},
    io::Errno,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::LibraryRoot;

const MIN_LABEL_CONFIDENCE: f64 = 0.15;
const MAX_LABELS: usize = 32;
const MAX_CACHED_OBSERVATIONS: usize = 128;
const MAX_PALETTE_CLUSTERS: usize = 32;
const VISUAL_CACHE_DIR: &str = "visual-assets";
const STAGING_PREFIX: &str = ".stage-";
const STAGING_SUFFIX: &str = ".tmp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualAsset {
    pub reading_id: String,
    pub title: String,
    pub relative_path: String,
    pub absolute_file_path: String,
    pub content_hash: String,
    pub(crate) media_dimensions: Option<crate::media_dimensions::MediaDimensions>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualAnalysisTask {
    pub reading_id: String,
    pub relative_path: String,
    pub absolute_file_path: String,
    pub content_hash: String,
    pub analyzer_version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingVisualAnalysis {
    pub tasks: Vec<VisualAnalysisTask>,
    pub hydrated_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualLabel {
    pub identifier: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightedColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualAnalysisResult {
    pub supported: bool,
    pub labels: Vec<VisualLabel>,
    pub palette: Vec<WeightedColor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredominantColor {
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

impl PredominantColor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Purple => "purple",
            Self::Pink => "pink",
            Self::Brown => "brown",
            Self::Black => "black",
            Self::Gray => "gray",
            Self::White => "white",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CachedVisualProjection {
    pub analyzer_version: Option<String>,
    pub visual_terms: String,
    pub predominant_color: Option<String>,
}

/// Create and validate the per-device visual staging directory beside the DB.
/// The final component is never followed when opened, so a planted symlink is
/// rejected rather than becoming an escape from Application Support.
pub(crate) fn prepare_visual_cache(db_path: &Path) -> Result<PathBuf> {
    let parent = db_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()?.join(parent)
    };
    let root = parent.join(VISUAL_CACHE_DIR);
    match rustix::fs::mkdir(&root, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
        Ok(()) => {}
        Err(Errno::EXIST) => {}
        Err(error) => return Err(error.into()),
    }
    let directory = open_cache_directory(&root)?;
    rustix::fs::fchmod(&directory, Mode::RUSR | Mode::WUSR | Mode::XUSR)?;
    cleanup_stale_temps(&directory)?;
    Ok(root)
}

fn open_cache_directory(root: &Path) -> Result<rustix::fd::OwnedFd> {
    Ok(rustix::fs::open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?)
}

fn cleanup_stale_temps(directory: &rustix::fd::OwnedFd) -> Result<()> {
    for entry in rustix::fs::Dir::read_from(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Ok(name_text) = name.to_str() else {
            continue;
        };
        if name_text.starts_with(STAGING_PREFIX) && name_text.ends_with(STAGING_SUFFIX) {
            match rustix::fs::unlinkat(directory, name, AtFlags::empty()) {
                Ok(()) | Err(Errno::NOENT) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    rustix::fs::fsync(directory)?;
    Ok(())
}

/// Remove content-addressed snapshots no longer referenced by the current
/// readings projection. Only exact 64-hex cache names are eligible; temp files
/// have their own cleanup path and unrelated files are left untouched.
pub(crate) fn prune_visual_cache(conn: &Connection, root: &Path) -> Result<()> {
    prune_visual_cache_files(root, &active_visual_hashes(conn)?)
}

pub(crate) fn active_visual_hashes(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT visual_asset_hash FROM readings WHERE visual_asset_hash IS NOT NULL",
    )?;
    let active: HashSet<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(active)
}

pub(crate) fn prune_visual_cache_files(root: &Path, active: &HashSet<String>) -> Result<()> {
    let directory = open_cache_directory(root)?;
    let mut changed = false;
    for entry in rustix::fs::Dir::read_from(&directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Ok(name_text) = name.to_str() else {
            continue;
        };
        if validate_content_hash(name_text).is_ok() && !active.contains(name_text) {
            match rustix::fs::unlinkat(&directory, name, AtFlags::empty()) {
                Ok(()) => changed = true,
                Err(Errno::NOENT) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    if changed {
        rustix::fs::fsync(&directory)?;
    }
    Ok(())
}

struct NormalizedAnalysis {
    labels: Vec<VisualLabel>,
    palette: Vec<WeightedColor>,
    visual_terms: String,
    predominant_color: Option<String>,
}

/// Open the selected preview one component at a time, reject symlinks below
/// the chosen library root, and hash the bytes actually read from the pinned FD.
pub(crate) fn inspect_asset(
    library: &LibraryRoot,
    reading_id: &str,
    relative_path: &str,
) -> Result<VisualAsset> {
    inspect_asset_inner(library, reading_id, relative_path, false, true)
}

/// Revalidate a machine-tag write using the same safe opener, without invoking
/// the visual staging test hook a second time for one completed analysis.
pub(crate) fn inspect_asset_for_tagging(
    library: &LibraryRoot,
    reading_id: &str,
    relative_path: &str,
) -> Result<VisualAsset> {
    inspect_asset_inner(library, reading_id, relative_path, false, false)
}

pub(crate) fn inspect_image_asset_for_tagging(
    library: &LibraryRoot,
    reading_id: &str,
    relative_path: &str,
) -> Result<VisualAsset> {
    inspect_asset_inner(library, reading_id, relative_path, true, false)
}

/// Hash an image preview and read its display dimensions from one pinned file
/// descriptor, so an external replacement cannot mix two asset versions.
pub(crate) fn inspect_image_asset(
    library: &LibraryRoot,
    reading_id: &str,
    relative_path: &str,
) -> Result<VisualAsset> {
    inspect_asset_inner(library, reading_id, relative_path, true, true)
}

fn inspect_asset_inner(
    library: &LibraryRoot,
    reading_id: &str,
    relative_path: &str,
    include_dimensions: bool,
    notify_test_hook: bool,
) -> Result<VisualAsset> {
    #[cfg(test)]
    if notify_test_hook {
        VISUAL_IO_TEST_HOOK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook();
            }
        });
    }
    #[cfg(not(test))]
    let _ = notify_test_hook;
    let mut file = open_source_asset(library, reading_id, relative_path)?;
    let media_dimensions = include_dimensions
        .then(|| crate::media_dimensions::image_dimensions(&mut file))
        .flatten();
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let absolute = library.reading_dir(reading_id).join(relative_path);
    Ok(VisualAsset {
        reading_id: reading_id.to_string(),
        title: String::new(),
        relative_path: relative_path.to_string(),
        absolute_file_path: absolute.to_string_lossy().into_owned(),
        content_hash: hex::encode(hasher.finalize()),
        media_dimensions,
    })
}

/// Read display-oriented dimensions from a reading's local movie asset.
/// `media_url` must retain the exact `cuttings-asset:assets/<file>` binding
/// found in the pinned article metadata before any bytes are inspected.
pub(crate) fn inspect_video_dimensions(
    library: &LibraryRoot,
    reading_id: &str,
    media_url: &str,
) -> Result<Option<crate::media_dimensions::MediaDimensions>> {
    let relative_path = media_url
        .strip_prefix("cuttings-asset:")
        .ok_or_else(|| anyhow::anyhow!("video media is not a local asset for {reading_id}"))?;
    let mut file = open_bound_asset(
        library,
        reading_id,
        relative_path,
        AssetBinding::Media(media_url),
    )?;
    Ok(crate::media_dimensions::video_dimensions(&mut file))
}

fn open_source_asset(
    library: &LibraryRoot,
    reading_id: &str,
    relative_path: &str,
) -> Result<fs::File> {
    open_bound_asset(
        library,
        reading_id,
        relative_path,
        AssetBinding::Preview(relative_path),
    )
}

#[derive(Clone, Copy)]
pub(crate) enum AssetBinding<'a> {
    Preview(&'a str),
    Media(&'a str),
}

pub(crate) fn open_bound_asset(
    library: &LibraryRoot,
    reading_id: &str,
    relative_path: &str,
    binding: AssetBinding<'_>,
) -> Result<fs::File> {
    validate_reading_id(reading_id)?;
    let file_name = validate_asset_path(relative_path)?;
    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let root = rustix::fs::open(
        library.path(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let articles = rustix::fs::openat(&root, "articles", directory_flags, Mode::empty())?;
    let prefix = reading_id.get(..2).unwrap_or(reading_id);
    let bucket = rustix::fs::openat(&articles, prefix, directory_flags, Mode::empty())?;
    let reading_dir = rustix::fs::openat(&bucket, reading_id, directory_flags, Mode::empty())?;

    // Validate the public file contract while the directory is pinned. A stale
    // DB row must not turn this into an arbitrary asset reader.
    let article_fd = rustix::fs::openat(
        &reading_dir,
        "article.md",
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let mut article = fs::File::from(article_fd);
    if !article.metadata()?.is_file() {
        bail!("article is not a regular file for {reading_id}");
    }
    let mut markdown = String::new();
    article.read_to_string(&mut markdown)?;
    let metadata = crate::parse_reading(&markdown)?.metadata;
    let binding_matches = match binding {
        AssetBinding::Preview(expected) => metadata.preview_asset.as_deref() == Some(expected),
        AssetBinding::Media(expected) => metadata.media_url.as_deref() == Some(expected),
    };
    if metadata.id != reading_id || !binding_matches {
        bail!("asset no longer belongs to reading {reading_id}");
    }

    let assets = rustix::fs::openat(&reading_dir, "assets", directory_flags, Mode::empty())?;
    let asset_fd = rustix::fs::openat(
        &assets,
        file_name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let file = fs::File::from(asset_fd);
    if !file.metadata()?.is_file() {
        bail!("preview asset is not a regular file for {reading_id}");
    }
    Ok(file)
}

/// Materialize scanner-identified bytes into the app-owned cache and return
/// that immutable path. Creation reads only from the pinned library FD and is
/// committed by atomic rename after its raw hash matches `content_hash`.
fn staged_asset(
    cache_root: &Path,
    library: &LibraryRoot,
    reading_id: &str,
    title: String,
    relative_path: &str,
    content_hash: &str,
) -> Result<VisualAsset> {
    #[cfg(test)]
    VISUAL_IO_TEST_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
    validate_content_hash(content_hash)?;
    let directory = open_cache_directory(cache_root)?;
    let target = content_hash;
    match rustix::fs::openat(
        &directory,
        target,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(fd) => {
            let file = fs::File::from(fd);
            if !file.metadata()?.is_file() {
                bail!("staged visual asset is not a regular file: {content_hash}");
            }
            rustix::fs::fchmod(&file, Mode::RUSR)?;
            return Ok(staged_asset_record(
                cache_root,
                reading_id,
                title,
                relative_path,
                content_hash,
            ));
        }
        Err(Errno::NOENT) => {}
        Err(error) => return Err(error.into()),
    }

    let mut source = open_source_asset(library, reading_id, relative_path)?;
    let temp_name = format!("{STAGING_PREFIX}{}{STAGING_SUFFIX}", crate::new_id());
    let stage_result = (|| -> Result<()> {
        let temp_fd = rustix::fs::openat(
            &directory,
            temp_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )?;
        let mut staged = fs::File::from(temp_fd);
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            staged.write_all(&buffer[..read])?;
        }
        if hex::encode(hasher.finalize()) != content_hash {
            bail!("preview asset changed after it was scanned");
        }
        staged.sync_all()?;
        rustix::fs::fchmod(&staged, Mode::RUSR)?;
        drop(staged);
        rustix::fs::renameat(&directory, temp_name.as_str(), &directory, target)?;
        rustix::fs::fsync(&directory)?;
        Ok(())
    })();
    if stage_result.is_err() {
        let _ = rustix::fs::unlinkat(&directory, temp_name.as_str(), AtFlags::empty());
    }
    stage_result?;

    Ok(staged_asset_record(
        cache_root,
        reading_id,
        title,
        relative_path,
        content_hash,
    ))
}

// A deterministic pause at the filesystem boundary lets the public Database
// concurrency tests hold a slow disk operation without depending on disk speed.
#[cfg(test)]
thread_local! {
    static VISUAL_IO_TEST_HOOK: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
pub(crate) fn with_visual_io_test_hook<T>(
    hook: impl FnMut() + 'static,
    work: impl FnOnce() -> T,
) -> T {
    VISUAL_IO_TEST_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    let result = work();
    VISUAL_IO_TEST_HOOK.with(|slot| *slot.borrow_mut() = None);
    result
}

fn staged_asset_record(
    cache_root: &Path,
    reading_id: &str,
    title: String,
    relative_path: &str,
    content_hash: &str,
) -> VisualAsset {
    VisualAsset {
        reading_id: reading_id.to_string(),
        title,
        relative_path: relative_path.to_string(),
        absolute_file_path: cache_root.join(content_hash).to_string_lossy().into_owned(),
        content_hash: content_hash.to_string(),
        media_dimensions: None,
    }
}

fn validate_content_hash(content_hash: &str) -> Result<()> {
    if content_hash.len() != 64
        || !content_hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("invalid visual content hash");
    }
    Ok(())
}

fn validate_reading_id(reading_id: &str) -> Result<()> {
    if reading_id.is_empty() || !reading_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("invalid reading id: {reading_id:?}");
    }
    Ok(())
}

fn validate_asset_path(path: &str) -> Result<&str> {
    let mut parts = path.split('/');
    let first = parts.next();
    let file = parts.next();
    if first != Some("assets")
        || file.is_none_or(|name| {
            name.is_empty() || name == "." || name == ".." || name.contains(['\\', '\0'])
        })
        || parts.next().is_some()
    {
        bail!("preview asset must be a direct child of assets/");
    }
    Ok(file.expect("checked above"))
}

pub fn current_visual_assets(
    conn: &Connection,
    library: &LibraryRoot,
    cache_root: &Path,
) -> Result<Vec<VisualAsset>> {
    prune_visual_cache(conn, cache_root)?;
    stage_visual_assets(current_visual_asset_candidates(conn)?, library, cache_root)
}

pub(crate) type VisualAssetCandidate = (String, String, String, String);

pub(crate) fn current_visual_asset_candidates(
    conn: &Connection,
) -> Result<Vec<VisualAssetCandidate>> {
    visual_asset_candidate_batch(conn, None, i64::MAX as usize)
}

pub(crate) fn visual_asset_candidate_batch(
    conn: &Connection,
    after_reading_id: Option<&str>,
    limit: usize,
) -> Result<Vec<VisualAssetCandidate>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, visual_asset_path, visual_asset_hash FROM readings
         WHERE id > ?1 AND visual_asset_path IS NOT NULL AND visual_asset_hash IS NOT NULL
         ORDER BY id LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(
            params![after_reading_id.unwrap_or(""), limit as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}

pub(crate) fn stage_visual_assets(
    rows: Vec<VisualAssetCandidate>,
    library: &LibraryRoot,
    cache_root: &Path,
) -> Result<Vec<VisualAsset>> {
    let mut assets = Vec::new();
    for (id, title, path, content_hash) in rows {
        match staged_asset(cache_root, library, &id, title, &path, &content_hash) {
            Ok(asset) => assets.push(asset),
            Err(error) => eprintln!("visual index: could not stage {id}: {error}"),
        }
    }
    Ok(assets)
}

/// Return analyzer-versioned work, hydrating exact cache hits as a side effect.
pub fn pending_visual_analysis(
    conn: &Connection,
    library: &LibraryRoot,
    cache_root: &Path,
    analyzer_version: &str,
    limit: usize,
) -> Result<PendingVisualAnalysis> {
    prune_visual_cache(conn, cache_root)?;
    let (candidates, hydrated_count) = pending_visual_candidates(conn, analyzer_version, limit)?;
    stage_visual_analysis(
        candidates,
        hydrated_count,
        library,
        cache_root,
        analyzer_version,
    )
}

pub(crate) fn pending_visual_candidates(
    conn: &Connection,
    analyzer_version: &str,
    limit: usize,
) -> Result<(Vec<VisualAssetCandidate>, usize)> {
    if analyzer_version.trim().is_empty() {
        bail!("analyzer version must not be blank");
    }
    let tx = conn.unchecked_transaction()?;
    // Exact cache hits need no file I/O: the indexed hash was produced by the
    // scanner's safe open, and completion also revalidated it. This makes
    // repeated small batches O(batch), not O(library size) asset reads.
    let hydrated_count = conn.execute(
        "UPDATE readings SET
           visual_analyzer_version=?1,
           visual_terms=COALESCE((
             SELECT CASE WHEN supported=1 THEN visual_terms ELSE '' END
             FROM visual_analysis a
             WHERE a.content_hash=readings.visual_asset_hash
               AND a.analyzer_version=?1
           ), ''),
           predominant_color=(
             SELECT CASE WHEN supported=1 THEN predominant_color ELSE NULL END
             FROM visual_analysis a
             WHERE a.content_hash=readings.visual_asset_hash
               AND a.analyzer_version=?1
           )
         WHERE visual_asset_hash IS NOT NULL AND EXISTS (
           SELECT 1 FROM visual_analysis a
           WHERE a.content_hash=readings.visual_asset_hash
             AND a.analyzer_version=?1
         ) AND visual_analyzer_version IS NOT ?1",
        params![analyzer_version],
    )?;
    let mut stmt = conn.prepare(
        "SELECT id, title, visual_asset_path, visual_asset_hash
         FROM readings r
         WHERE visual_asset_path IS NOT NULL AND visual_asset_hash IS NOT NULL
           AND NOT EXISTS (
             SELECT 1 FROM visual_analysis a
             WHERE a.content_hash=r.visual_asset_hash AND a.analyzer_version=?1
           )
           AND r.id=(
             SELECT MIN(r2.id) FROM readings r2
             WHERE r2.visual_asset_hash=r.visual_asset_hash
               AND r2.visual_asset_path IS NOT NULL
           )
         ORDER BY id LIMIT ?2",
    )?;
    let candidates = stmt
        .query_map(params![analyzer_version, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    tx.commit()?;
    Ok((candidates, hydrated_count))
}

pub(crate) fn stage_visual_analysis(
    candidates: Vec<VisualAssetCandidate>,
    hydrated_count: usize,
    library: &LibraryRoot,
    cache_root: &Path,
    analyzer_version: &str,
) -> Result<PendingVisualAnalysis> {
    let mut pending = Vec::new();
    for (id, title, path, content_hash) in candidates {
        match staged_asset(cache_root, library, &id, title, &path, &content_hash) {
            Ok(asset) => pending.push(VisualAnalysisTask {
                reading_id: asset.reading_id,
                relative_path: asset.relative_path,
                absolute_file_path: asset.absolute_file_path,
                content_hash: asset.content_hash,
                analyzer_version: analyzer_version.to_string(),
            }),
            Err(error) => eprintln!("visual index: could not stage task for {id}: {error}"),
        }
    }
    Ok(PendingVisualAnalysis {
        tasks: pending,
        hydrated_count,
    })
}

/// Walk a bounded window of reading identities, including cache hits and
/// unreadable assets. Advancing over that window prevents failed media from
/// starving later work and keeps hydration inside the same small DB budget.
pub(crate) fn pending_visual_candidate_batch(
    conn: &Connection,
    analyzer_version: &str,
    after_reading_id: Option<&str>,
    limit: usize,
) -> Result<(Vec<VisualAssetCandidate>, usize, Option<String>)> {
    if analyzer_version.trim().is_empty() {
        bail!("analyzer version must not be blank");
    }
    let transaction = conn.unchecked_transaction()?;
    let rows = visual_asset_candidate_batch(conn, after_reading_id, limit)?;
    let next = (rows.len() == limit).then(|| rows.last().unwrap().0.clone());
    let mut pending = Vec::new();
    let mut hydrated = 0;
    for (id, title, path, hash) in rows {
        let cached: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT CASE WHEN supported=1 THEN visual_terms ELSE '' END,
                    CASE WHEN supported=1 THEN predominant_color ELSE NULL END
             FROM visual_analysis WHERE content_hash=?1 AND analyzer_version=?2",
                params![hash, analyzer_version],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((terms, color)) = cached {
            hydrated += conn.execute(
                "UPDATE readings SET visual_analyzer_version=?2, visual_terms=?3, predominant_color=?4
                 WHERE id=?1 AND visual_analyzer_version IS NOT ?2",
                params![id, analyzer_version, terms, color],
            )?;
        } else {
            let first: Option<String> = conn.query_row(
                "SELECT MIN(id) FROM readings WHERE visual_asset_hash=?1 AND visual_asset_path IS NOT NULL",
                [&hash], |row| row.get(0),
            )?;
            if first.as_deref() == Some(id.as_str()) {
                pending.push((id, title, path, hash));
            }
        }
    }
    transaction.commit()?;
    Ok((pending, hydrated, next))
}

/// Complete a task only if the same safe path still contains the same bytes.
/// Returns `false` for ordinary stale work so clients can simply discard it.
pub fn complete_visual_analysis(
    conn: &Connection,
    library: &LibraryRoot,
    task: &VisualAnalysisTask,
    result: &VisualAnalysisResult,
) -> Result<bool> {
    let Some(current) = verify_visual_analysis(library, task)? else {
        return Ok(false);
    };
    complete_verified_visual_analysis(conn, &current, task, result)
}

pub(crate) fn verify_visual_analysis(
    library: &LibraryRoot,
    task: &VisualAnalysisTask,
) -> Result<Option<VisualAsset>> {
    if task.analyzer_version.trim().is_empty() {
        bail!("analyzer version must not be blank");
    }
    Ok(
        match inspect_asset(library, &task.reading_id, &task.relative_path) {
            Ok(asset) if asset.content_hash == task.content_hash => Some(asset),
            Ok(_) | Err(_) => None,
        },
    )
}

pub(crate) fn complete_verified_visual_analysis(
    conn: &Connection,
    current: &VisualAsset,
    task: &VisualAnalysisTask,
    result: &VisualAnalysisResult,
) -> Result<bool> {
    // File verification happens before acquiring the DB mutex. A concurrent
    // reconciliation may have advanced the index meanwhile; never restore the
    // old asset identity or attach its analysis to a newer reading revision.
    let tx = conn.unchecked_transaction()?;
    let matches_index: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM readings WHERE id=?1
         AND preview_asset=?2 AND visual_asset_hash=?3)",
        params![task.reading_id, task.relative_path, task.content_hash],
        |row| row.get(0),
    )?;
    if !matches_index {
        return Ok(false);
    }
    let normalized = if result.supported {
        normalize_result(result)?
    } else {
        NormalizedAnalysis {
            labels: Vec::new(),
            palette: Vec::new(),
            visual_terms: String::new(),
            predominant_color: None,
        }
    };
    let labels_json = serde_json::to_string(&normalized.labels)?;
    let palette_json = serde_json::to_string(&normalized.palette)?;
    conn.execute(
        "INSERT INTO visual_analysis
         (content_hash, analyzer_version, supported, labels_json, palette_json,
          visual_terms, predominant_color, completed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch())
         ON CONFLICT(content_hash, analyzer_version) DO UPDATE SET
           supported=excluded.supported,
           labels_json=excluded.labels_json,
           palette_json=excluded.palette_json,
           visual_terms=excluded.visual_terms,
           predominant_color=excluded.predominant_color,
           completed_at=excluded.completed_at",
        params![
            current.content_hash,
            task.analyzer_version,
            result.supported as i32,
            labels_json,
            palette_json,
            normalized.visual_terms,
            normalized.predominant_color
        ],
    )?;
    conn.execute(
        "UPDATE readings SET visual_analyzer_version=?2, visual_terms=?3,
             predominant_color=?4
         WHERE visual_asset_hash=?1",
        params![
            current.content_hash,
            task.analyzer_version,
            normalized.visual_terms,
            normalized.predominant_color
        ],
    )?;
    tx.commit()?;
    Ok(true)
}

fn normalize_result(result: &VisualAnalysisResult) -> Result<NormalizedAnalysis> {
    if result
        .labels
        .iter()
        .any(|label| !label.confidence.is_finite() || !(0.0..=1.0).contains(&label.confidence))
    {
        bail!("label confidences must be finite values from 0...1");
    }
    let mut labels: Vec<VisualLabel> = result
        .labels
        .iter()
        .filter_map(|label| {
            let normalized = normalize_identifier(&label.identifier);
            (!normalized.is_empty()).then_some(VisualLabel {
                identifier: normalized,
                confidence: label.confidence,
            })
        })
        .collect();
    labels.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.identifier.as_str().cmp(b.identifier.as_str()))
    });
    let mut seen = HashSet::new();
    let labels: Vec<VisualLabel> = labels
        .into_iter()
        .filter(|label| seen.insert(label.identifier.clone()))
        .take(MAX_CACHED_OBSERVATIONS)
        .collect();
    let palette = normalize_palette(&result.palette)?;
    let color = predominant_color(&palette)?.map(|value| value.as_str().to_string());
    let mut terms: Vec<String> = labels
        .iter()
        .filter(|label| label.confidence >= MIN_LABEL_CONFIDENCE)
        .take(MAX_LABELS)
        .map(|label| label.identifier.clone())
        .collect();
    if let Some(color) = color.as_ref() {
        terms.push(color.clone());
    }
    Ok(NormalizedAnalysis {
        labels,
        palette,
        visual_terms: terms.join(" "),
        predominant_color: color,
    })
}

pub(crate) fn normalize_palette(palette: &[WeightedColor]) -> Result<Vec<WeightedColor>> {
    for sample in palette {
        validate_color(sample)?;
    }
    let mut palette: Vec<WeightedColor> = palette
        .iter()
        .filter(|sample| sample.weight > 0.0)
        .cloned()
        .collect();
    palette.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then_with(|| b.red.total_cmp(&a.red))
            .then_with(|| b.green.total_cmp(&a.green))
            .then_with(|| b.blue.total_cmp(&a.blue))
    });
    palette.truncate(MAX_PALETTE_CLUSTERS);
    let total: f64 = palette.iter().map(|sample| sample.weight).sum();
    if total > 0.0 {
        for sample in &mut palette {
            sample.weight /= total;
        }
    }
    Ok(palette)
}

fn normalize_identifier(identifier: &str) -> String {
    identifier
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Choose a stable palette cluster for the flat media placeholder.
///
/// Neutral canvas colours are not useful highlights. Aggregate the remaining
/// clusters through the same colour-family classifier used by visual search,
/// then return the largest exact cluster inside the winning family.
pub(crate) fn representative_placeholder_color(palette: &[WeightedColor]) -> Option<WeightedColor> {
    let valid = |sample: &&WeightedColor| sample.weight > 0.0 && validate_color(sample).is_ok();
    let by_coverage = |lhs: &&WeightedColor, rhs: &&WeightedColor| {
        lhs.weight
            .total_cmp(&rhs.weight)
            .then_with(|| lhs.red.total_cmp(&rhs.red))
            .then_with(|| lhs.green.total_cmp(&rhs.green))
            .then_with(|| lhs.blue.total_cmp(&rhs.blue))
    };
    let highlight_family = predominant_color_matching(palette, is_chromatic)
        .ok()
        .flatten();
    palette
        .iter()
        .filter(valid)
        .filter(|sample| {
            highlight_family
                .is_some_and(|family| color_family(sample.red, sample.green, sample.blue) == family)
        })
        .max_by(by_coverage)
        .or_else(|| palette.iter().filter(valid).max_by(by_coverage))
        .cloned()
}

pub fn predominant_color(palette: &[WeightedColor]) -> Result<Option<PredominantColor>> {
    predominant_color_matching(palette, |_| true)
}

fn predominant_color_matching(
    palette: &[WeightedColor],
    include: fn(PredominantColor) -> bool,
) -> Result<Option<PredominantColor>> {
    let mut weights: BTreeMap<&'static str, (PredominantColor, f64)> = BTreeMap::new();
    for sample in palette {
        validate_color(sample)?;
        if sample.weight == 0.0 {
            continue;
        }
        let family = color_family(sample.red, sample.green, sample.blue);
        if !include(family) {
            continue;
        }
        weights
            .entry(family.as_str())
            .and_modify(|(_, weight)| *weight += sample.weight)
            .or_insert((family, sample.weight));
    }
    Ok(weights
        .into_values()
        .max_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then_with(|| b.0.as_str().cmp(a.0.as_str()))
        })
        .map(|(family, _)| family))
}

fn is_chromatic(family: PredominantColor) -> bool {
    !matches!(
        family,
        PredominantColor::Black | PredominantColor::Gray | PredominantColor::White
    )
}

fn validate_color(sample: &WeightedColor) -> Result<()> {
    if !sample.red.is_finite()
        || !sample.green.is_finite()
        || !sample.blue.is_finite()
        || !sample.weight.is_finite()
        || !(0.0..=1.0).contains(&sample.red)
        || !(0.0..=1.0).contains(&sample.green)
        || !(0.0..=1.0).contains(&sample.blue)
        || sample.weight < 0.0
    {
        bail!("palette values must be finite RGB 0...1 with nonnegative weights");
    }
    Ok(())
}

fn color_family(red: f64, green: f64, blue: f64) -> PredominantColor {
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let delta = max - min;
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    if max <= 0.14 {
        return PredominantColor::Black;
    }
    if min >= 0.88 && saturation <= 0.12 {
        return PredominantColor::White;
    }
    if saturation <= 0.16 {
        return PredominantColor::Gray;
    }
    let mut hue = if max == red {
        60.0 * ((green - blue) / delta)
    } else if max == green {
        60.0 * (((blue - red) / delta) + 2.0)
    } else {
        60.0 * (((red - green) / delta) + 4.0)
    };
    if hue < 0.0 {
        hue += 360.0;
    }
    if (15.0..55.0).contains(&hue) && max < 0.68 {
        PredominantColor::Brown
    } else if !(15.0..345.0).contains(&hue) {
        PredominantColor::Red
    } else if hue < 45.0 {
        PredominantColor::Orange
    } else if hue < 70.0 {
        PredominantColor::Yellow
    } else if hue < 165.0 {
        PredominantColor::Green
    } else if hue < 255.0 {
        PredominantColor::Blue
    } else if hue < 300.0 {
        PredominantColor::Purple
    } else {
        PredominantColor::Pink
    }
}

/// Latest supported projection for a content hash, used while rebuilding the
/// readings table. Exact-version pending work can subsequently replace it.
pub(crate) fn cached_projection(
    conn: &Connection,
    content_hash: &str,
) -> Result<CachedVisualProjection> {
    conn.query_row(
        "SELECT analyzer_version,
                CASE WHEN supported=1 THEN visual_terms ELSE '' END,
                CASE WHEN supported=1 THEN predominant_color ELSE NULL END
         FROM visual_analysis
         WHERE content_hash=?1
         ORDER BY completed_at DESC, rowid DESC LIMIT 1",
        params![content_hash],
        |row| {
            Ok(CachedVisualProjection {
                analyzer_version: row.get(0)?,
                visual_terms: row.get(1)?,
                predominant_color: row.get(2)?,
            })
        },
    )
    .optional()
    .map(|value| value.unwrap_or_default())
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use crate::{
        apply_diffs, diff, get_reading, import_image, list_readings, open_index, rebuild,
        scan_library, view_counts, CountScope, ListOptions, PredominantColor,
    };

    fn setup_image(bytes: &[u8]) -> (TempDir, LibraryRoot, Connection, String, PathBuf) {
        let dir = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let imported = import_image(&library, bytes.to_vec(), "image/png", "Dining setup").unwrap();
        let index_path = dir.path().join("index.db");
        let conn = open_index(&index_path).unwrap();
        let cache_root = prepare_visual_cache(&index_path).unwrap();
        rebuild(&conn, &library).unwrap();
        (dir, library, conn, imported.id, cache_root)
    }

    fn supported_result() -> VisualAnalysisResult {
        VisualAnalysisResult {
            supported: true,
            labels: vec![
                VisualLabel {
                    identifier: "Dining_Room".into(),
                    confidence: 0.92,
                },
                VisualLabel {
                    identifier: "chair".into(),
                    confidence: 0.81,
                },
                VisualLabel {
                    identifier: "weak_noise".into(),
                    confidence: 0.01,
                },
            ],
            palette: vec![
                WeightedColor {
                    red: 0.9,
                    green: 0.1,
                    blue: 0.1,
                    weight: 1.0,
                },
                WeightedColor {
                    red: 0.1,
                    green: 0.25,
                    blue: 0.9,
                    weight: 3.0,
                },
            ],
        }
    }

    #[test]
    fn normalizes_vision_identifiers_and_applies_confidence_floor() {
        let result = VisualAnalysisResult {
            supported: true,
            labels: vec![
                VisualLabel {
                    identifier: "Dining_Room".into(),
                    confidence: 0.9,
                },
                VisualLabel {
                    identifier: "CHAIR".into(),
                    confidence: 0.8,
                },
                VisualLabel {
                    identifier: "noise".into(),
                    confidence: 0.149,
                },
            ],
            palette: vec![],
        };
        let normalized = normalize_result(&result).unwrap();
        assert_eq!(
            normalized
                .labels
                .iter()
                .map(|label| label.identifier.as_str())
                .collect::<Vec<_>>(),
            ["dining room", "chair", "noise"]
        );
        assert_eq!(normalized.labels[0].confidence, 0.9);
        assert_eq!(normalized.visual_terms, "dining room chair");
    }

    #[test]
    fn palette_weights_choose_stable_family() {
        let palette = vec![
            WeightedColor {
                red: 0.95,
                green: 0.1,
                blue: 0.1,
                weight: 0.2,
            },
            WeightedColor {
                red: 0.1,
                green: 0.25,
                blue: 0.9,
                weight: 0.5,
            },
            WeightedColor {
                red: 0.2,
                green: 0.35,
                blue: 0.8,
                weight: 0.4,
            },
        ];
        assert_eq!(
            predominant_color(&palette).unwrap(),
            Some(PredominantColor::Blue)
        );
    }

    #[test]
    fn placeholder_color_uses_the_chromatic_highlight_over_a_white_canvas() {
        let blue = WeightedColor {
            red: 0.1,
            green: 0.3,
            blue: 0.9,
            weight: 0.08,
        };
        let palette = vec![
            WeightedColor {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                weight: 0.92,
            },
            blue.clone(),
        ];

        assert_eq!(representative_placeholder_color(&palette), Some(blue));
    }

    #[test]
    fn placeholder_color_uses_the_largest_cluster_in_the_winning_chromatic_family() {
        let strongest_blue = WeightedColor {
            red: 0.1,
            green: 0.3,
            blue: 0.9,
            weight: 0.25,
        };
        let palette = vec![
            WeightedColor {
                red: 0.9,
                green: 0.1,
                blue: 0.1,
                weight: 0.30,
            },
            strongest_blue.clone(),
            WeightedColor {
                red: 0.15,
                green: 0.35,
                blue: 0.8,
                weight: 0.23,
            },
            WeightedColor {
                red: 0.2,
                green: 0.4,
                blue: 0.85,
                weight: 0.22,
            },
        ];

        assert_eq!(
            representative_placeholder_color(&palette),
            Some(strongest_blue)
        );
    }

    #[test]
    fn placeholder_color_keeps_a_neutral_when_the_image_has_no_chromatic_family() {
        let black = WeightedColor {
            red: 0.02,
            green: 0.02,
            blue: 0.02,
            weight: 0.8,
        };
        let palette = vec![
            black.clone(),
            WeightedColor {
                red: 0.45,
                green: 0.45,
                blue: 0.45,
                weight: 0.2,
            },
        ];

        assert_eq!(representative_placeholder_color(&palette), Some(black));
    }

    #[test]
    fn scanner_hashes_raw_bytes_and_exposes_only_safe_assets() {
        let (_dir, library, conn, id, cache_root) = setup_image(b"raw preview bytes");
        let assets = current_visual_assets(&conn, &library, &cache_root).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].reading_id, id);
        assert_eq!(
            assets[0].content_hash,
            crate::sha256_hex(b"raw preview bytes")
        );
        assert!(assets[0].relative_path.starts_with("assets/"));
        assert!(Path::new(&assets[0].absolute_file_path).starts_with(&cache_root));
        assert_eq!(
            std::fs::read(&assets[0].absolute_file_path).unwrap(),
            b"raw preview bytes"
        );

        let reused = current_visual_assets(&conn, &library, &cache_root).unwrap();
        assert_eq!(reused[0].absolute_file_path, assets[0].absolute_file_path);
        assert!(!std::fs::read_dir(&cache_root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(STAGING_PREFIX)
        }));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&cache_root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&assets[0].absolute_file_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o400
            );
        }
    }

    #[test]
    fn completion_indexes_labels_and_color_and_rebuild_reuses_cache() {
        let (_dir, library, conn, id, cache_root) = setup_image(b"semantic image");
        let task = pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 10)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        assert!(complete_visual_analysis(&conn, &library, &task, &supported_result()).unwrap());
        let cached_labels: String = conn
            .query_row(
                "SELECT labels_json FROM visual_analysis WHERE content_hash=?1",
                params![task.content_hash],
                |row| row.get(0),
            )
            .unwrap();
        let cached_labels: Vec<VisualLabel> = serde_json::from_str(&cached_labels).unwrap();
        assert_eq!(cached_labels[0].identifier, "dining room");
        assert_eq!(cached_labels[0].confidence, 0.92);
        let cached_palette: String = conn
            .query_row(
                "SELECT palette_json FROM visual_analysis WHERE content_hash=?1",
                params![task.content_hash],
                |row| row.get(0),
            )
            .unwrap();
        let cached_palette: Vec<WeightedColor> = serde_json::from_str(&cached_palette).unwrap();
        assert_eq!(cached_palette.len(), 2);
        assert_eq!(cached_palette[0].blue, 0.9);
        assert_eq!(cached_palette[0].weight, 0.75);
        assert_eq!(cached_palette[1].weight, 0.25);

        let chair = list_readings(
            &conn,
            &ListOptions {
                query: Some("chair".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(chair.iter().map(|row| &row.id).collect::<Vec<_>>(), [&id]);
        assert_eq!(
            chair[0].dominant_color,
            Some(WeightedColor {
                red: 0.1,
                green: 0.25,
                blue: 0.9,
                weight: 0.75,
            })
        );
        let blue = list_readings(
            &conn,
            &ListOptions {
                predominant_color: Some(PredominantColor::Blue),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(blue.iter().map(|row| &row.id).collect::<Vec<_>>(), [&id]);
        assert_eq!(blue[0].dominant_color, chair[0].dominant_color);
        assert_eq!(
            get_reading(&conn, &id).unwrap().unwrap().0.dominant_color,
            chair[0].dominant_color
        );
        assert_eq!(
            list_readings(
                &conn,
                &ListOptions {
                    query: Some("blue".into()),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            view_counts(
                &conn,
                &CountScope {
                    predominant_color: Some(PredominantColor::Blue),
                    ..Default::default()
                }
            )
            .unwrap()
            .all,
            1
        );

        rebuild(&conn, &library).unwrap();
        assert!(
            pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 10)
                .unwrap()
                .tasks
                .is_empty()
        );
        let cache_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM visual_analysis", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cache_count, 1);
        let rebuilt = list_readings(
            &conn,
            &ListOptions {
                query: Some("dining room".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rebuilt.len(), 1);
        assert_eq!(rebuilt[0].dominant_color, chair[0].dominant_color);
    }

    #[test]
    fn white_canvas_does_not_hide_a_meaningful_placeholder_color() {
        let (_dir, library, conn, id, cache_root) = setup_image(b"white canvas image");
        let task = pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        let result = VisualAnalysisResult {
            supported: true,
            labels: vec![],
            palette: vec![
                WeightedColor {
                    red: 1.0,
                    green: 1.0,
                    blue: 1.0,
                    weight: 0.7,
                },
                WeightedColor {
                    red: 0.15,
                    green: 0.35,
                    blue: 0.85,
                    weight: 0.3,
                },
            ],
        };
        assert!(complete_visual_analysis(&conn, &library, &task, &result).unwrap());

        let row = list_readings(&conn, &ListOptions::default())
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(row.id, id);
        assert_eq!(
            row.dominant_color,
            Some(WeightedColor {
                red: 0.15,
                green: 0.35,
                blue: 0.85,
                weight: 0.3,
            })
        );
    }

    #[test]
    fn exact_cache_hit_reports_hydration_and_restores_dominant_color() {
        let (_dir, library, conn, id, cache_root) = setup_image(b"cached visual analysis");
        let task = pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        assert!(complete_visual_analysis(&conn, &library, &task, &supported_result()).unwrap());

        conn.execute(
            "UPDATE readings SET visual_analyzer_version=NULL,
                                 visual_terms='',
                                 predominant_color=NULL
             WHERE id=?1",
            params![id],
        )
        .unwrap();
        assert_eq!(
            list_readings(&conn, &ListOptions::default()).unwrap()[0].dominant_color,
            None
        );

        let pending =
            pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 10).unwrap();
        assert!(pending.tasks.is_empty());
        assert_eq!(pending.hydrated_count, 1);
        assert_eq!(
            list_readings(&conn, &ListOptions::default()).unwrap()[0].dominant_color,
            Some(WeightedColor {
                red: 0.1,
                green: 0.25,
                blue: 0.9,
                weight: 0.75,
            })
        );

        let already_hydrated =
            pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 10).unwrap();
        assert!(already_hydrated.tasks.is_empty());
        assert_eq!(already_hydrated.hydrated_count, 0);
    }

    #[test]
    fn malformed_cached_palette_never_breaks_reading_queries() {
        let (_dir, library, conn, id, cache_root) = setup_image(b"malformed palette cache");
        let task = pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        assert!(complete_visual_analysis(&conn, &library, &task, &supported_result()).unwrap());
        for malformed_palette in ["{malformed", "[42]"] {
            conn.execute(
                "UPDATE visual_analysis SET palette_json=?1
                 WHERE content_hash=?2 AND analyzer_version=?3",
                params![malformed_palette, task.content_hash, task.analyzer_version],
            )
            .unwrap();

            let listed = list_readings(&conn, &ListOptions::default()).unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].dominant_color, None);
            let searched = list_readings(
                &conn,
                &ListOptions {
                    query: Some("chair".into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(searched.len(), 1);
            assert_eq!(searched[0].dominant_color, None);
            assert_eq!(
                get_reading(&conn, &id).unwrap().unwrap().0.dominant_color,
                None
            );
        }
    }

    #[test]
    fn unsupported_hash_is_not_retried_until_analyzer_changes() {
        let (_dir, library, conn, _id, cache_root) = setup_image(b"unsupported image");
        let older = pending_visual_analysis(&conn, &library, &cache_root, "vision-r1", 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        assert!(complete_visual_analysis(&conn, &library, &older, &supported_result()).unwrap());
        let task = pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        let unsupported = VisualAnalysisResult {
            supported: false,
            labels: vec![],
            palette: vec![],
        };
        assert!(complete_visual_analysis(&conn, &library, &task, &unsupported).unwrap());
        assert!(
            pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 1)
                .unwrap()
                .tasks
                .is_empty()
        );
        assert_eq!(
            pending_visual_analysis(&conn, &library, &cache_root, "vision-r3", 1)
                .unwrap()
                .tasks
                .len(),
            1
        );
        rebuild(&conn, &library).unwrap();
        assert_eq!(
            list_readings(&conn, &ListOptions::default()).unwrap()[0].dominant_color,
            None
        );
        assert!(list_readings(
            &conn,
            &ListOptions {
                query: Some("chair".into()),
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn changed_or_deleted_asset_invalidates_projection_and_stale_completion() {
        let (_dir, library, conn, id, cache_root) = setup_image(b"old bytes");
        let old_scan = scan_library(&library).unwrap();
        let task = pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        let old_staged_path = task.absolute_file_path.clone();
        let canonical_path = library.reading_dir(&id).join(&task.relative_path);
        std::fs::write(&canonical_path, b"new bytes").unwrap();
        let new_scan = scan_library(&library).unwrap();
        apply_diffs(&conn, &diff(&old_scan, &new_scan)).unwrap();
        assert!(!complete_visual_analysis(&conn, &library, &task, &supported_result()).unwrap());
        let replacement = pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        let replacement_staged_path = replacement.absolute_file_path.clone();
        assert_ne!(replacement.content_hash, task.content_hash);

        std::fs::remove_file(&canonical_path).unwrap();
        let after_delete = scan_library(&library).unwrap();
        apply_diffs(&conn, &diff(&new_scan, &after_delete)).unwrap();
        let state: (Option<String>, String) = conn
            .query_row(
                "SELECT visual_asset_hash, visual_terms FROM readings WHERE id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, (None, String::new()));
        assert!(current_visual_assets(&conn, &library, &cache_root)
            .unwrap()
            .is_empty());
        assert!(!Path::new(&old_staged_path).exists());
        assert!(!Path::new(&replacement_staged_path).exists());
    }

    #[cfg(unix)]
    #[test]
    fn post_scan_symlink_swap_cannot_change_returned_staged_bytes() {
        use std::os::unix::fs::symlink;

        let (dir, library, conn, id, cache_root) = setup_image(b"inside bytes");
        let asset = current_visual_assets(&conn, &library, &cache_root)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            std::fs::read(&asset.absolute_file_path).unwrap(),
            b"inside bytes"
        );
        assert!(Path::new(&asset.absolute_file_path).starts_with(&cache_root));

        let canonical_path = library.reading_dir(&id).join(&asset.relative_path);
        let outside = dir.path().join("outside.png");
        std::fs::write(&outside, b"outside bytes").unwrap();
        std::fs::remove_file(&canonical_path).unwrap();
        symlink(&outside, &canonical_path).unwrap();

        // Vision and Spotlight receive only the app-owned staged URL. Swapping
        // the canonical library entry after scan cannot redirect that URL.
        assert_eq!(
            std::fs::read(&asset.absolute_file_path).unwrap(),
            b"inside bytes"
        );
        let task = pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 1)
            .unwrap()
            .tasks
            .pop()
            .unwrap();
        assert_eq!(task.absolute_file_path, asset.absolute_file_path);
        assert_eq!(
            std::fs::read(&task.absolute_file_path).unwrap(),
            b"inside bytes"
        );
        assert!(!complete_visual_analysis(&conn, &library, &task, &supported_result()).unwrap());

        rebuild(&conn, &library).unwrap();
        assert!(current_visual_assets(&conn, &library, &cache_root)
            .unwrap()
            .is_empty());
        assert!(
            pending_visual_analysis(&conn, &library, &cache_root, "vision-r2", 10)
                .unwrap()
                .tasks
                .is_empty()
        );
        assert!(!Path::new(&asset.absolute_file_path).exists());
    }

    #[test]
    fn cache_initialization_cleans_only_stale_temps() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");
        let root = prepare_visual_cache(&db_path).unwrap();
        let stale = root.join(format!("{STAGING_PREFIX}abandoned{STAGING_SUFFIX}"));
        let unrelated = root.join("keep-me");
        std::fs::write(&stale, b"partial").unwrap();
        std::fs::write(&unrelated, b"unrelated").unwrap();

        assert_eq!(prepare_visual_cache(&db_path).unwrap(), root);
        assert!(!stale.exists());
        assert!(unrelated.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cache_rejects_root_symlinks_and_unlinks_temp_symlinks_without_following() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        let db_path = dir.path().join("index.db");
        symlink(outside_dir.path(), dir.path().join(VISUAL_CACHE_DIR)).unwrap();
        assert!(prepare_visual_cache(&db_path).is_err());

        std::fs::remove_file(dir.path().join(VISUAL_CACHE_DIR)).unwrap();
        let root = prepare_visual_cache(&db_path).unwrap();
        let outside_file = outside_dir.path().join("outside");
        std::fs::write(&outside_file, b"keep outside").unwrap();
        let temp_link = root.join(format!("{STAGING_PREFIX}link{STAGING_SUFFIX}"));
        symlink(&outside_file, &temp_link).unwrap();

        prepare_visual_cache(&db_path).unwrap();
        assert!(!temp_link.exists());
        assert_eq!(std::fs::read(outside_file).unwrap(), b"keep outside");
    }
}
