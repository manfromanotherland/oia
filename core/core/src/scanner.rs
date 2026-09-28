// SPDX-License-Identifier: MIT

use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

use anyhow::Result;
use rustix::fs::{Mode, OFlags};

use crate::{parse_reading, types::LibraryRoot, visual_index::VisualAsset, Metadata};

/// One article as seen on disk.
#[derive(Debug, Clone)]
pub struct ScannedReading {
    pub id: String,
    pub path: PathBuf,
    pub source_hash: String,
    pub modified_at: SystemTime,
    pub metadata: Metadata,
    /// Whether the reading folder contains a regular personal-note sidecar.
    pub has_note: bool,
    /// Safely opened preview bytes, identified by their raw SHA-256 hash.
    pub visual_asset: Option<VisualAsset>,
    /// Display width divided by display height for standalone local media.
    /// Derived from headers during reconciliation; never synced or authoritative.
    pub media_aspect_ratio: Option<f64>,
    /// Raw Markdown body (after stripping frontmatter), stored for FTS indexing.
    pub body: String,
}

/// Change detected between two scans.
#[derive(Debug)]
pub enum ScanDiff {
    Added(ScannedReading),
    Changed(ScannedReading),
    Removed(String),
}

/// Walk `articles/` and return one entry per `.md` file.
///
/// Files that fail to parse are skipped with an error logged to stderr rather
/// than aborting the whole scan, so a single corrupt file doesn't break the
/// indexer.
pub fn scan_library(library: &LibraryRoot) -> Result<Vec<ScannedReading>> {
    let mut readings = Vec::new();
    for directory in reading_directories(library)? {
        if let Some(reading) = scan_reading_directory(library, &directory)? {
            readings.push(reading);
        }
    }
    Ok(readings)
}

/// Enumerate candidate reading folders without opening Markdown or preview
/// bytes. A staged reconciliation can keep this list and inspect a few folders
/// at a time while the UI remains responsive.
pub(crate) fn reading_directories(library: &LibraryRoot) -> Result<Vec<PathBuf>> {
    let articles_dir = library.articles_dir();
    if !articles_dir.is_dir() {
        return Ok(vec![]);
    }

    let mut directories = Vec::new();
    // Fan-out layout: articles/<2-char prefix>/<id>/article.md. Each reading is a
    // self-contained folder, so walk two levels of sub-directories (bucket → the
    // reading folder) and read the `article.md` inside each reading folder. The
    // sibling `assets/` folder and `highlights.md` are not readings, so keying on
    // the fixed `article.md` filename skips them without extra checks.
    for bucket in std::fs::read_dir(&articles_dir)? {
        let bucket = bucket?;
        if !bucket.file_type()?.is_dir() {
            continue; // ignore stray files sitting directly in articles/
        }
        for reading_dir in std::fs::read_dir(bucket.path())? {
            let reading_dir = reading_dir?;
            if !reading_dir.file_type()?.is_dir() {
                continue; // ignore stray files sitting directly in a bucket
            }
            directories.push(reading_dir.path());
        }
    }

    Ok(directories)
}

/// Resolve precise file events into reading folders. Ancestor/root/unknown
/// events deliberately request a full scan; no event can create a path escape.
pub(crate) fn reading_ids_for_changed_paths(
    library: &LibraryRoot,
    paths: &[String],
) -> Option<HashSet<String>> {
    let mut ids = HashSet::new();
    for path in paths {
        let relative = Path::new(path).strip_prefix(library.path()).ok()?;
        let parts = relative
            .components()
            .map(|component| match component {
                Component::Normal(value) => value.to_str(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        match parts.first().copied() {
            Some("inbox") => {} // Inbox ingestion separately requests reconciliation after saving.
            Some("articles") if parts.len() >= 3 => {
                let id = parts[2];
                if id.len() < 2
                    || !id.bytes().all(|value| value.is_ascii_alphanumeric())
                    || parts[1] != &id[..2]
                {
                    return None;
                }
                ids.insert(id.to_string());
            }
            _ => return None,
        }
    }
    Some(ids)
}

pub(crate) fn scan_reading_ids(
    library: &LibraryRoot,
    ids: &HashSet<String>,
) -> Result<Vec<ScannedReading>> {
    let mut readings = Vec::new();
    for id in ids {
        let directory = library.reading_dir(id);
        // Match the full scanner: never traverse a symlinked bucket or reading
        // directory supplied by a filesystem event.
        if !directory.parent().is_some_and(is_real_directory) || !is_real_directory(&directory) {
            continue;
        }
        if let Some(reading) = scan_reading_directory(library, &directory)? {
            readings.push(reading);
        }
    }
    Ok(readings)
}

/// Preview hashes cannot describe every reader asset or posterless movie.
/// Filesystem events invalidate those cached presentations without reading
/// entire movie/image contents solely to manufacture an index-row change.
pub(crate) fn changed_paths_include_assets(library: &LibraryRoot, paths: &[String]) -> bool {
    paths.iter().any(|path| {
        Path::new(path)
            .strip_prefix(library.articles_dir())
            .ok()
            .and_then(|relative| relative.components().nth(2))
            .is_some_and(|component| component.as_os_str() == "assets")
    })
}

fn is_real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

pub(crate) fn scan_reading_directory(
    library: &LibraryRoot,
    directory: &Path,
) -> Result<Option<ScannedReading>> {
    let path = directory.join("article.md");
    let mut article = match open_reading_article(library, directory)? {
        Some(file) => file,
        None => return Ok(None), // a folder without a safe article.md is not a reading
    };
    let modified_at = article.metadata()?.modified()?;
    let mut content = String::new();
    match article.read_to_string(&mut content) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("scanner: skipping {}: {e}", path.display());
            return Ok(None);
        }
    }
    let reading = match parse_reading(&content) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("scanner: skipping {}: {e}", path.display());
            return Ok(None);
        }
    };

    // The folder's location must match the reading's own identity:
    // `articles/<prefix>/<id>/`. If an external edit or sync drops an
    // article whose frontmatter id disagrees with its folder (or bucket),
    // skip it rather than index it — otherwise the index would point a
    // reading at a folder that `delete_reading` and asset resolution
    // (both keyed on the id) would not agree with.
    if directory != library.reading_dir(&reading.metadata.id) {
        eprintln!(
            "scanner: skipping {}: folder does not match frontmatter id {}",
            path.display(),
            reading.metadata.id
        );
        return Ok(None);
    }

    let visual_asset = reading
        .metadata
        .preview_asset
        .as_deref()
        .and_then(|relative_path| {
            let inspected = if matches!(
                reading.metadata.kind,
                crate::ReadingKind::Article | crate::ReadingKind::Image
            ) {
                crate::visual_index::inspect_image_asset(
                    library,
                    &reading.metadata.id,
                    relative_path,
                )
            } else {
                crate::visual_index::inspect_asset(library, &reading.metadata.id, relative_path)
            };
            match inspected {
                Ok(asset) => Some(asset),
                Err(error) => {
                    eprintln!(
                        "scanner: ignoring unsafe preview for {}: {error}",
                        reading.metadata.id
                    );
                    None
                }
            }
        });

    let media_aspect_ratio =
        inspect_media_aspect_ratio(library, &reading.metadata, visual_asset.as_ref());

    Ok(Some(ScannedReading {
        id: reading.metadata.id.clone(),
        source_hash: reading.metadata.source_hash.clone(),
        modified_at,
        path,
        has_note: note_file_exists(library, &reading.metadata.id),
        visual_asset,
        media_aspect_ratio,
        metadata: reading.metadata,
        body: reading.body,
    }))
}

/// Pin each directory component before reading Markdown. A staged scan may
/// pause after enumeration, while an external sync can replace a folder; do
/// not follow a newly inserted symlink out of the selected library.
fn open_reading_article(library: &LibraryRoot, directory: &Path) -> Result<Option<std::fs::File>> {
    let Ok(relative) = directory.strip_prefix(library.articles_dir()) else {
        return Ok(None);
    };
    let mut parts = relative.components();
    let (Some(Component::Normal(bucket)), Some(Component::Normal(reading)), None) =
        (parts.next(), parts.next(), parts.next())
    else {
        return Ok(None);
    };
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let root = rustix::fs::open(
        library.path(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let Ok(articles) = rustix::fs::openat(&root, "articles", flags, Mode::empty()) else {
        return Ok(None);
    };
    let Ok(bucket) = rustix::fs::openat(&articles, bucket, flags, Mode::empty()) else {
        return Ok(None);
    };
    let Ok(reading) = rustix::fs::openat(&bucket, reading, flags, Mode::empty()) else {
        return Ok(None);
    };
    let Ok(article) = rustix::fs::openat(
        &reading,
        "article.md",
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) else {
        return Ok(None);
    };
    let file = std::fs::File::from(article);
    Ok(file.metadata()?.is_file().then_some(file))
}

pub(crate) fn inspect_media_aspect_ratio(
    library: &LibraryRoot,
    metadata: &Metadata,
    visual_asset: Option<&VisualAsset>,
) -> Option<f64> {
    let inspected = match metadata.kind {
        crate::ReadingKind::Article | crate::ReadingKind::Image => {
            return visual_asset
                .and_then(|asset| asset.media_dimensions)
                .and_then(|dimensions| dimensions.aspect_ratio());
        }
        crate::ReadingKind::Video => metadata.media_url.as_deref().map(|media_url| {
            crate::visual_index::inspect_video_dimensions(library, &metadata.id, media_url)
        }),
        crate::ReadingKind::Quote => None,
    }?;

    match inspected {
        Ok(dimensions) => dimensions.and_then(|value| value.aspect_ratio()),
        Err(error) => {
            eprintln!(
                "scanner: could not inspect media dimensions for {}: {error}",
                metadata.id
            );
            None
        }
    }
}

/// Diff two snapshots, using body text, frontmatter metadata, and note presence
/// as change signals.
///
/// Items present in `new` but absent in `old` → `Added`.
/// Items present in both but with differing body or metadata → `Changed`.
/// Items present in `old` but absent in `new` → `Removed`.
pub fn diff(old: &[ScannedReading], new: &[ScannedReading]) -> Vec<ScanDiff> {
    let old_readings: HashMap<&str, &ScannedReading> =
        old.iter().map(|r| (r.id.as_str(), r)).collect();
    let new_ids: std::collections::HashSet<&str> = new.iter().map(|r| r.id.as_str()).collect();

    let mut diffs = Vec::new();

    for reading in new {
        match old_readings.get(reading.id.as_str()) {
            None => diffs.push(ScanDiff::Added(reading.clone())),
            Some(old_reading)
                if old_reading.source_hash != reading.source_hash
                    || old_reading.body != reading.body
                    || old_reading.metadata != reading.metadata
                    || old_reading.has_note != reading.has_note
                    || old_reading.visual_asset != reading.visual_asset
                    || old_reading.media_aspect_ratio != reading.media_aspect_ratio =>
            {
                diffs.push(ScanDiff::Changed(reading.clone()))
            }
            _ => {}
        }
    }

    for reading in old {
        if !new_ids.contains(reading.id.as_str()) {
            diffs.push(ScanDiff::Removed(reading.id.clone()));
        }
    }

    diffs
}

/// Whether a reading has a valid note sidecar for indexing purposes.
///
/// The public note API refuses symlinks, so the derived cache must not advertise
/// one as a personal note either. Other metadata errors are treated as absence;
/// a later filesystem event will retry the scan.
pub(crate) fn note_file_exists(library: &LibraryRoot, reading_id: &str) -> bool {
    let path = library.note_path(reading_id);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata.file_type().is_file(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            eprintln!("scanner: could not inspect {}: {error}", path.display());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    use crate::{new_id, write_reading, LibraryRoot, Metadata};

    fn sample_metadata(id: &str, url: &str) -> Metadata {
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
            excerpt: None,
            word_count: None,
            lang: None,
            source_hash: String::new(),
        }
    }

    fn make_library(dir: &TempDir) -> LibraryRoot {
        fs::create_dir_all(dir.path().join("articles")).unwrap();
        LibraryRoot::new(dir.path()).unwrap()
    }

    #[test]
    fn empty_library_returns_empty_scan() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let results = scan_library(&lib).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn scan_returns_one_entry_per_article() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        let id1 = new_id();
        let id2 = new_id();
        write_reading(
            &lib,
            sample_metadata(&id1, "https://a.com"),
            "body one".to_string(),
        )
        .unwrap();
        write_reading(
            &lib,
            sample_metadata(&id2, "https://b.com"),
            "body two".to_string(),
        )
        .unwrap();

        let results = scan_library(&lib).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn scan_skips_reading_whose_folder_mismatches_its_id() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        // An article whose frontmatter id is B, written into A's folder.
        let a = new_id();
        let b = new_id();
        let folder_a = lib.reading_dir(&a);
        fs::create_dir_all(&folder_a).unwrap();
        let content = crate::render_reading(&crate::Reading {
            metadata: sample_metadata(&b, "https://b.com"),
            body: "body".into(),
        })
        .unwrap();
        fs::write(folder_a.join("article.md"), content).unwrap();

        // The folder (A) disagrees with the frontmatter id (B), so it is skipped.
        assert!(scan_library(&lib).unwrap().is_empty());
    }

    #[test]
    fn scan_populates_source_hash() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        let id = new_id();
        let reading = write_reading(
            &lib,
            sample_metadata(&id, "https://example.com"),
            "hello".to_string(),
        )
        .unwrap();

        let results = scan_library(&lib).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_hash, reading.metadata.source_hash);
    }

    #[test]
    fn scan_reads_article_preview_aspect_ratio_from_local_asset() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let id = new_id();
        let mut metadata = sample_metadata(&id, "https://example.com/article");
        metadata.preview_asset = Some("assets/social.png".into());
        write_reading(&lib, metadata, "body".into()).unwrap();
        let mut png = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        png.extend_from_slice(&1200_u32.to_be_bytes());
        png.extend_from_slice(&800_u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        fs::create_dir_all(lib.assets_dir(&id)).unwrap();
        fs::write(lib.assets_dir(&id).join("social.png"), png).unwrap();

        let results = scan_library(&lib).unwrap();

        assert_eq!(results[0].media_aspect_ratio, Some(3.0 / 2.0));
    }

    #[test]
    fn diff_detects_added() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            sample_metadata(&id, "https://example.com"),
            "body".to_string(),
        )
        .unwrap();

        let new_scan = scan_library(&lib).unwrap();
        let diffs = diff(&[], &new_scan);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0], ScanDiff::Added(_)));
    }

    #[test]
    fn diff_detects_removed() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            sample_metadata(&id, "https://example.com"),
            "body".to_string(),
        )
        .unwrap();
        let old_scan = scan_library(&lib).unwrap();

        // Delete the file to simulate removal.
        fs::remove_file(lib.article_path(&id)).unwrap();

        let new_scan = scan_library(&lib).unwrap();
        let diffs = diff(&old_scan, &new_scan);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(&diffs[0], ScanDiff::Removed(rid) if rid == &id));
    }

    #[test]
    fn diff_detects_changed() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            sample_metadata(&id, "https://example.com"),
            "original body".to_string(),
        )
        .unwrap();
        let old_scan = scan_library(&lib).unwrap();

        // Overwrite with different body → different source_hash.
        write_reading(
            &lib,
            sample_metadata(&id, "https://example.com"),
            "updated body".to_string(),
        )
        .unwrap();
        let new_scan = scan_library(&lib).unwrap();

        let diffs = diff(&old_scan, &new_scan);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0], ScanDiff::Changed(_)));
    }

    #[test]
    fn diff_no_change_when_hash_matches() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        let id = new_id();
        write_reading(
            &lib,
            sample_metadata(&id, "https://example.com"),
            "body".to_string(),
        )
        .unwrap();
        let scan = scan_library(&lib).unwrap();

        // Diff against itself — nothing should change.
        let diffs = diff(&scan, &scan);
        assert!(diffs.is_empty());
    }

    #[test]
    fn diff_detects_media_metadata_change_without_body_change() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(
            &lib,
            sample_metadata(&id, "https://example.com/gallery"),
            "same body".to_string(),
        )
        .unwrap();
        let old_scan = scan_library(&lib).unwrap();

        let mut metadata = sample_metadata(&id, "https://example.com/gallery");
        metadata.kind = crate::ReadingKind::Image;
        metadata.media_url = Some("https://cdn.example.com/photo.jpg".into());
        metadata.preview_asset = Some("assets/photo.jpg".into());
        write_reading(&lib, metadata, "same body".to_string()).unwrap();
        let new_scan = scan_library(&lib).unwrap();

        let diffs = diff(&old_scan, &new_scan);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0], ScanDiff::Changed(_)));
    }

    #[test]
    fn scan_and_diff_track_personal_note_presence() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(
            &lib,
            sample_metadata(&id, "https://example.com/noted"),
            "same body".to_string(),
        )
        .unwrap();

        let without_note = scan_library(&lib).unwrap();
        assert!(!without_note[0].has_note);

        fs::write(lib.note_path(&id), "A personal note").unwrap();
        let with_note = scan_library(&lib).unwrap();
        assert!(with_note[0].has_note);
        assert!(matches!(
            diff(&without_note, &with_note)[..],
            [ScanDiff::Changed(_)]
        ));

        fs::remove_file(lib.note_path(&id)).unwrap();
        let removed_note = scan_library(&lib).unwrap();
        assert!(!removed_note[0].has_note);
        assert!(matches!(
            diff(&with_note, &removed_note)[..],
            [ScanDiff::Changed(_)]
        ));
    }

    #[test]
    fn scan_rehashes_preview_when_bytes_change_without_size_or_mtime_change() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let id = new_id();
        let mut metadata = sample_metadata(&id, "https://example.com/image");
        metadata.kind = crate::ReadingKind::Image;
        metadata.preview_asset = Some("assets/preview.bin".into());
        write_reading(&lib, metadata, "same body".into()).unwrap();
        fs::create_dir_all(lib.assets_dir(&id)).unwrap();
        let asset = lib.assets_dir(&id).join("preview.bin");
        fs::write(&asset, b"before").unwrap();
        let modified = fs::metadata(&asset).unwrap().modified().unwrap();
        let first = scan_library(&lib).unwrap();

        fs::write(&asset, b"after!").unwrap();
        fs::File::open(&asset)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let second = scan_library(&lib).unwrap();

        assert_eq!(fs::metadata(&asset).unwrap().len(), 6);
        assert_eq!(fs::metadata(&asset).unwrap().modified().unwrap(), modified);
        assert_ne!(first[0].visual_asset, second[0].visual_asset);
        assert!(matches!(diff(&first, &second)[..], [ScanDiff::Changed(_)]));
    }

    #[cfg(unix)]
    #[test]
    fn enumerated_reading_replaced_by_symlink_is_not_scanned() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let id = new_id();
        write_reading(
            &lib,
            sample_metadata(&id, "https://example.com/safe"),
            "safe body".into(),
        )
        .unwrap();
        let directory = reading_directories(&lib).unwrap().pop().unwrap();
        let moved = dir.path().join("moved-reading");
        fs::rename(&directory, &moved).unwrap();
        symlink(&moved, &directory).unwrap();

        assert!(scan_reading_directory(&lib, &directory).unwrap().is_none());
    }
}
