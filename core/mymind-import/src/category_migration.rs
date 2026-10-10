// SPDX-License-Identifier: MIT

//! Explicit, recoverable categorization of existing lightweight links.

use crate::{link_metadata::HostLimiter, product_migration::prepare_backup_directory};
use anyhow::{bail, Context, Result};
use oia_core::{
    fetch_category_artwork, fetch_link_metadata, parse_reading, sha256_hex,
    update_link_category_if_unchanged, url_id, CategoryUpdateOutcome, DetectedLinkMetadata,
    ImageBytes, LibraryRoot, LinkCategory, ReadingKind,
};
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
};
use url::Url;

#[derive(Debug, Clone)]
pub struct EnrichExistingCategoriesOptions {
    pub library: PathBuf,
    pub write: bool,
    pub verbose: bool,
    pub workers: usize,
    pub expected_count: Option<usize>,
    pub expected_digest: Option<String>,
    pub backup_dir: Option<PathBuf>,
    /// Optional explicit subset from a reviewed preview; IDs must be eligible.
    pub reading_ids: Option<Vec<String>>,
}

#[derive(Debug, Default)]
pub struct EnrichExistingCategoriesReport {
    pub planned: usize,
    pub target_digest: String,
    pub fetched: usize,
    pub recognized: usize,
    pub books: usize,
    pub music: usize,
    pub movies: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub fetch_failures: usize,
    pub artwork_available: usize,
    pub artwork_created: usize,
    pub artwork_failures: usize,
    pub errors: usize,
    pub backup_dir: Option<PathBuf>,
    write: bool,
    lines: Vec<String>,
}

impl EnrichExistingCategoriesReport {
    pub fn render(&self) -> String {
        let mut output = format!("link category migration{}\nTarget links: {}\nTarget digest: {}\nCategory evidence: fetched {}, recognized {} (books {}, music {}, movies {}), unavailable {}\nArtwork: available {}, created {}, unavailable {}\n",
            if self.write { "" } else { " preview" }, self.planned, self.target_digest, self.fetched, self.recognized, self.books, self.music, self.movies, self.fetch_failures,
            self.artwork_available, self.artwork_created, self.artwork_failures);
        if let Some(backup) = &self.backup_dir {
            output.push_str(&format!(
                "Original files and report: {}\n",
                backup.display()
            ));
        }
        for line in &self.lines {
            output.push_str(line);
            output.push('\n');
        }
        output.push_str(&format!("\nSummary: target links {}, categorized {}, changed {}, unchanged {}, unavailable {}, errors {}\n", self.planned, self.recognized, self.changed, self.unchanged, self.fetch_failures, self.errors));
        if !self.write {
            output.push_str("No library files or assets were written. Re-run with --write and --backup-dir to apply.\n");
        }
        output
    }
}

struct CategoryTarget {
    id: String,
    url: String,
    original: Vec<u8>,
    fingerprint: String,
    needs_preview: bool,
}

trait CategoryFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Option<DetectedLinkMetadata>>;
    fn artwork(&self, url: &str) -> Result<ImageBytes>;
}

struct HttpCategoryFetcher {
    hosts: HostLimiter,
}
impl CategoryFetcher for HttpCategoryFetcher {
    fn fetch(&self, url: &str) -> Result<Option<DetectedLinkMetadata>> {
        let parsed = Url::parse(url)?;
        let _permit = self
            .hosts
            .acquire(parsed.host_str().context("category URL has no host")?);
        // Core validates public DNS at every redirect and pins approved addresses.
        // Its narrow known-route fallback works even when page retrieval fails.
        fetch_link_metadata(url).or_else(|_| fetch_link_metadata(url))
    }
    fn artwork(&self, url: &str) -> Result<ImageBytes> {
        let parsed = Url::parse(url)?;
        let _permit = self
            .hosts
            .acquire(parsed.host_str().context("artwork URL has no host")?);
        fetch_category_artwork(url)
    }
}

pub fn enrich_existing_categories(
    options: EnrichExistingCategoriesOptions,
) -> Result<EnrichExistingCategoriesReport> {
    enrich_existing_categories_with_fetcher(
        options,
        Arc::new(HttpCategoryFetcher {
            hosts: HostLimiter::new(2),
        }),
    )
}

fn enrich_existing_categories_with_fetcher(
    options: EnrichExistingCategoriesOptions,
    fetcher: Arc<dyn CategoryFetcher>,
) -> Result<EnrichExistingCategoriesReport> {
    let library_path = options
        .library
        .canonicalize()
        .context("could not resolve the library folder")?;
    if !library_path.is_dir() {
        bail!("Óia library is not a directory");
    }
    let library = LibraryRoot::new(&library_path)?;
    let mut targets = category_targets(&library)?;
    if let Some(ids) = &options.reading_ids {
        if ids.is_empty() {
            bail!("category ID selection must not be empty");
        }
        let mut selected = HashSet::new();
        for id in ids {
            if id.len() != 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                bail!("category ID selection requires one 64-character lowercase hexadecimal reading ID per line");
            }
            if !selected.insert(id.as_str()) {
                bail!("category ID selection contains a duplicate reading ID");
            }
        }
        let eligible: HashSet<_> = targets.iter().map(|target| target.id.as_str()).collect();
        if selected.iter().any(|id| !eligible.contains(id)) {
            bail!("category ID selection contains a missing or ineligible reading");
        }
        targets.retain(|target| selected.contains(target.id.as_str()));
    }
    let digest = target_digest(&targets);
    if options
        .expected_count
        .is_some_and(|count| count != targets.len())
    {
        bail!(
            "target snapshot changed: expected {} links but found {}",
            options.expected_count.unwrap(),
            targets.len()
        );
    }
    if let Some(expected) = options.expected_digest.as_deref() {
        let expected = expected.trim().to_ascii_lowercase();
        if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("expected target digest must be exactly 64 hexadecimal characters");
        }
        if expected != digest {
            bail!("target snapshot changed: digest does not match");
        }
    }
    let backup = if options.write {
        Some(prepare_backup_directory(
            &library_path,
            options.backup_dir.as_deref(),
        )?)
    } else {
        None
    };
    let mut report = EnrichExistingCategoriesReport {
        planned: targets.len(),
        target_digest: digest,
        backup_dir: backup.clone(),
        write: options.write,
        ..Default::default()
    };
    if let Some(backup) = backup.as_deref() {
        save_report(backup, &report)?;
    }
    let total = targets.len();
    let queue = Arc::new(Mutex::new(VecDeque::from(targets)));
    let (sender, receiver) = mpsc::channel();
    thread::scope(|scope| -> Result<()> {
        for _ in 0..options.workers.clamp(1, 32).min(total.max(1)) {
            let queue = Arc::clone(&queue);
            let fetcher = Arc::clone(&fetcher);
            let sender = sender.clone();
            scope.spawn(move || loop {
                let Some(target) = queue
                    .lock()
                    .expect("category queue lock poisoned")
                    .pop_front()
                else {
                    break;
                };
                let evidence = fetcher.fetch(&target.url);
                let mut artwork_failed = false;
                let artwork = if options.write && target.needs_preview {
                    evidence
                        .as_ref()
                        .ok()
                        .and_then(|metadata| metadata.as_ref())
                        .and_then(|metadata| metadata.image_url.as_deref())
                        .and_then(|url| match fetcher.artwork(url) {
                            Ok(image) => Some(image),
                            Err(_) => {
                                artwork_failed = true;
                                None
                            }
                        })
                } else {
                    None
                };
                if sender
                    .send((target, evidence, artwork, artwork_failed))
                    .is_err()
                {
                    break;
                }
            });
        }
        drop(sender);
        for (index, (target, evidence, artwork, artwork_failed)) in receiver.into_iter().enumerate()
        {
            report.artwork_failures += usize::from(artwork_failed);
            match evidence {
                Ok(Some(metadata)) => {
                    report.fetched += 1;
                    report.recognized += 1;
                    match metadata.category {
                        LinkCategory::Book => report.books += 1,
                        LinkCategory::Music => report.music += 1,
                        LinkCategory::Movie => report.movies += 1,
                    }
                    report.artwork_available +=
                        usize::from(target.needs_preview && metadata.image_url.is_some());
                    if options.verbose {
                        report.lines.push(format!(
                            "CATEGORY reading {}: {}",
                            target.id,
                            metadata.category.as_str()
                        ));
                    }
                    if let Some(backup) = backup.as_deref() {
                        // Record potential asset hashes before applying, including
                        // errors after installation, so recovery can identify them.
                        if let Some(image) = &artwork {
                            report.lines.push(format!(
                                "ARTWORK CANDIDATE reading {}: sha256 {}",
                                target.id,
                                sha256_hex(&image.bytes)
                            ));
                        }
                        save_report(backup, &report)?;
                        match apply_category(&library, backup, &target, metadata.category, artwork)
                        {
                            Ok(outcome) => {
                                if outcome.changed {
                                    report.changed += 1;
                                } else {
                                    report.unchanged += 1;
                                }
                                if let Some(asset) = outcome.created_asset {
                                    report.artwork_created += 1;
                                    report.lines.push(format!(
                                        "ARTWORK reading {}: {} sha256 {}{}",
                                        target.id,
                                        asset,
                                        outcome.asset_sha256.as_deref().unwrap_or(""),
                                        if outcome.changed {
                                            ""
                                        } else {
                                            " retained after concurrent edit"
                                        }
                                    ));
                                }
                                if !outcome.changed && options.verbose {
                                    report.lines.push(format!(
                                        "UNCHANGED reading {}: edited while evidence was fetched",
                                        target.id
                                    ));
                                }
                            }
                            Err(_) => {
                                report.errors += 1;
                                report.lines.push(format!("ERROR reading {}: category could not be safely written; original retained in backup", target.id));
                            }
                        }
                        save_report(backup, &report)?;
                    }
                }
                Ok(None) => {
                    report.fetched += 1;
                    report.unchanged += 1;
                }
                Err(_) => {
                    report.fetch_failures += 1;
                    report.unchanged += 1;
                }
            }
            let processed = index + 1;
            if total >= 50 && (processed % 50 == 0 || processed == total) {
                eprintln!("Link categories: processed {processed}/{total}");
            }
        }
        Ok(())
    })?;
    report.lines.sort();
    if let Some(backup) = backup.as_deref() {
        save_report(backup, &report)?;
    }
    Ok(report)
}

fn save_report(backup: &Path, report: &EnrichExistingCategoriesReport) -> Result<()> {
    let mut staged = tempfile::NamedTempFile::new_in(backup)?;
    staged.write_all(report.render().as_bytes())?;
    staged.as_file().sync_all()?;
    staged
        .persist(backup.join("report.txt"))
        .map_err(|e| e.error)?;
    fs::File::open(backup)?.sync_all()?;
    Ok(())
}

fn category_targets(library: &LibraryRoot) -> Result<Vec<CategoryTarget>> {
    let mut targets = Vec::new();
    if !library.articles_dir().is_dir() {
        return Ok(targets);
    }
    for bucket in fs::read_dir(library.articles_dir())? {
        let bucket = bucket?;
        if !bucket.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(bucket.path())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path().join("article.md");
            if !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) {
                continue;
            }
            let Ok(original) = fs::read(&path) else {
                continue;
            };
            let Ok(text) = std::str::from_utf8(&original) else {
                continue;
            };
            let Ok(reading) = parse_reading(text) else {
                continue;
            };
            let metadata = reading.metadata;
            if metadata.kind != ReadingKind::Article
                || !metadata.lightweight
                || metadata
                    .source_profile
                    .as_ref()
                    .is_some_and(|source| source.source_type == "social_post")
                || metadata.link_category.is_some()
                || entry.path() != library.reading_dir(&metadata.id)
            {
                continue;
            }
            let Some(url) = crate::http_url(&metadata.url) else {
                continue;
            };
            if url_id(&url).ok().as_deref() != Some(metadata.id.as_str()) {
                continue;
            }
            let needs_preview = !metadata
                .preview_asset
                .as_deref()
                .is_some_and(|asset| safe_preview_exists(&entry.path(), asset));
            targets.push(CategoryTarget {
                id: metadata.id,
                url,
                fingerprint: sha256_hex(&original),
                original,
                needs_preview,
            });
        }
    }
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(targets)
}

fn safe_preview_exists(reading_dir: &Path, relative: &str) -> bool {
    let Some(name) = relative.strip_prefix("assets/") else {
        return false;
    };
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return false;
    }
    let assets = reading_dir.join("assets");
    fs::symlink_metadata(&assets).is_ok_and(|m| m.file_type().is_dir())
        && fs::symlink_metadata(assets.join(name))
            .is_ok_and(|m| m.file_type().is_file() && m.len() > 0)
}

fn target_digest(targets: &[CategoryTarget]) -> String {
    let mut joined = targets
        .iter()
        .map(|t| t.id.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    sha256_hex(joined.as_bytes())
}

fn apply_category(
    library: &LibraryRoot,
    backup: &Path,
    target: &CategoryTarget,
    category: LinkCategory,
    artwork: Option<ImageBytes>,
) -> Result<CategoryUpdateOutcome> {
    let path = backup.join(format!("{}.md", target.id));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&target.original)?;
    file.sync_all()?;
    drop(file);
    fs::File::open(backup)?.sync_all()?;
    match update_link_category_if_unchanged(
        library,
        &target.id,
        category,
        &target.fingerprint,
        artwork,
    ) {
        Ok(outcome) if !outcome.changed => {
            fs::remove_file(path)?;
            Ok(outcome)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oia_core::import_link;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    struct FixtureFetcher {
        calls: AtomicUsize,
        artwork_calls: AtomicUsize,
        fail_artwork: bool,
    }
    impl FixtureFetcher {
        fn new(fail_artwork: bool) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                artwork_calls: AtomicUsize::new(0),
                fail_artwork,
            }
        }
    }
    fn png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 96, 0, 2,
            0, 0, 5, 0, 1, 165, 246, 69, 64, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ]
    }
    impl CategoryFetcher for FixtureFetcher {
        fn fetch(&self, url: &str) -> Result<Option<DetectedLinkMetadata>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let category = if url.contains("book") {
                Some(LinkCategory::Book)
            } else if url.contains("music") {
                Some(LinkCategory::Music)
            } else if url.contains("movie") {
                Some(LinkCategory::Movie)
            } else {
                None
            };
            Ok(category.map(|category| DetectedLinkMetadata {
                category,
                title: Some("Untrusted replacement title".into()),
                image_url: Some("https://example.com/cover.png".into()),
                isbn: None,
            }))
        }
        fn artwork(&self, url: &str) -> Result<ImageBytes> {
            self.artwork_calls.fetch_add(1, Ordering::Relaxed);
            if self.fail_artwork {
                bail!("unavailable artwork");
            }
            Ok(ImageBytes {
                url: url.into(),
                content_type: "image/png".into(),
                bytes: png(),
            })
        }
    }
    fn options(
        library: &Path,
        backup: Option<&Path>,
        write: bool,
    ) -> EnrichExistingCategoriesOptions {
        EnrichExistingCategoriesOptions {
            library: library.into(),
            write,
            verbose: true,
            workers: 2,
            expected_count: None,
            expected_digest: None,
            backup_dir: backup.map(Path::to_path_buf),
            reading_ids: None,
        }
    }

    #[test]
    fn preview_includes_products_and_fetches_no_artwork_or_library_writes() {
        let dir = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let mut snapshots = Vec::new();
        for name in ["book", "music", "movie", "ordinary"] {
            let saved = import_link(&library, &format!("https://example.com/{name}")).unwrap();
            if name == "book" {
                let path = library.article_path(&saved.id);
                let original = fs::read_to_string(&path).unwrap();
                fs::write(&path, original.replacen("\n---\n", "\nproduct:\n  version: 1\n  price: '140'\n  currency: EUR\n  price_is_from: false\n---\n", 1)).unwrap();
            }
            snapshots.push((
                saved.id.clone(),
                fs::read(library.article_path(&saved.id)).unwrap(),
            ));
        }
        let fetcher = Arc::new(FixtureFetcher::new(false));
        let report = enrich_existing_categories_with_fetcher(
            options(dir.path(), None, false),
            fetcher.clone(),
        )
        .unwrap();
        assert_eq!(
            (report.planned, report.books, report.music, report.movies),
            (4, 1, 1, 1)
        );
        assert_eq!(report.changed, 0);
        assert_eq!(fetcher.artwork_calls.load(Ordering::Relaxed), 0);
        for (id, bytes) in snapshots {
            assert_eq!(fs::read(library.article_path(&id)).unwrap(), bytes);
            assert_eq!(fs::read_dir(library.assets_dir(&id)).unwrap().count(), 0);
        }
    }

    #[test]
    fn write_backs_up_exact_files_and_adds_only_category_and_missing_local_artwork() {
        let dir = TempDir::new().unwrap();
        let backup = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = import_link(&library, "https://example.com/book").unwrap();
        let original = fs::read_to_string(library.article_path(&saved.id)).unwrap().replacen("\n---\n", "\n# external writer\ncustom_field: keep\nproduct:\n  version: 1\n  price: '140'\n  currency: EUR\n  price_is_from: false\n---\n", 1);
        fs::write(library.article_path(&saved.id), &original).unwrap();
        fs::write(library.reading_dir(&saved.id).join("note.md"), "Keep note").unwrap();
        let fetcher = Arc::new(FixtureFetcher::new(false));
        let report = enrich_existing_categories_with_fetcher(
            options(dir.path(), Some(backup.path()), true),
            fetcher,
        )
        .unwrap();
        assert_eq!(
            (report.changed, report.artwork_created, report.errors),
            (1, 1, 0)
        );
        assert_eq!(
            fs::read(backup.path().join(format!("{}.md", saved.id))).unwrap(),
            original.as_bytes()
        );
        let original_reading = parse_reading(&original).unwrap();
        let updated_text = fs::read_to_string(library.article_path(&saved.id)).unwrap();
        let mut updated = parse_reading(&updated_text).unwrap();
        assert_eq!(
            updated.metadata.link_category.take().as_deref(),
            Some("book")
        );
        let preview = updated.metadata.preview_asset.take().unwrap();
        assert_eq!(
            fs::read(library.reading_dir(&saved.id).join(&preview)).unwrap(),
            png()
        );
        assert_eq!(updated.metadata, original_reading.metadata);
        assert_eq!(updated.body, original_reading.body);
        assert!(updated_text.contains("# external writer\ncustom_field: keep"));
        assert_eq!(
            fs::read_to_string(library.reading_dir(&saved.id).join("note.md")).unwrap(),
            "Keep note"
        );
        assert!(fs::read_to_string(backup.path().join("report.txt"))
            .unwrap()
            .contains(&format!("ARTWORK reading {}: {preview}", saved.id)));
        let second = enrich_existing_categories_with_fetcher(
            options(dir.path(), None, false),
            Arc::new(FixtureFetcher::new(false)),
        )
        .unwrap();
        assert_eq!(second.planned, 0);
    }

    #[test]
    fn existing_preview_is_preserved_and_unavailable_artwork_still_categorizes() {
        let dir = TempDir::new().unwrap();
        let backup = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = import_link(&library, "https://example.com/music").unwrap();
        let original = fs::read_to_string(library.article_path(&saved.id))
            .unwrap()
            .replacen("\n---\n", "\npreview_asset: assets/old.png\n---\n", 1);
        fs::write(library.article_path(&saved.id), original).unwrap();
        fs::write(library.assets_dir(&saved.id).join("old.png"), "Old artwork").unwrap();
        import_link(&library, "https://example.com/book").unwrap();
        let fetcher = Arc::new(FixtureFetcher::new(true));
        let report = enrich_existing_categories_with_fetcher(
            options(dir.path(), Some(backup.path()), true),
            fetcher.clone(),
        )
        .unwrap();
        assert_eq!(
            (
                report.changed,
                report.artwork_created,
                report.artwork_failures
            ),
            (2, 0, 1)
        );
        assert_eq!(fetcher.artwork_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            fs::read(library.assets_dir(&saved.id).join("old.png")).unwrap(),
            b"Old artwork"
        );
        assert_eq!(
            parse_reading(&fs::read_to_string(library.article_path(&saved.id)).unwrap())
                .unwrap()
                .metadata
                .preview_asset
                .as_deref(),
            Some("assets/old.png")
        );
    }

    #[test]
    fn snapshot_guards_run_before_network_and_backup_creation() {
        let dir = TempDir::new().unwrap();
        let backup = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        import_link(&library, "https://example.com/book").unwrap();
        let fetcher = Arc::new(FixtureFetcher::new(false));
        let mut args = options(dir.path(), Some(backup.path()), true);
        args.expected_count = Some(2);
        assert!(enrich_existing_categories_with_fetcher(args, fetcher.clone()).is_err());
        let mut args = options(dir.path(), Some(backup.path()), true);
        args.expected_digest = Some("0".repeat(64));
        assert!(enrich_existing_categories_with_fetcher(args, fetcher.clone()).is_err());
        assert_eq!(fetcher.calls.load(Ordering::Relaxed), 0);
        assert_eq!(fs::read_dir(backup.path()).unwrap().count(), 0);
    }

    struct ConcurrentFetcher {
        library: LibraryRoot,
    }
    impl CategoryFetcher for ConcurrentFetcher {
        fn fetch(&self, url: &str) -> Result<Option<DetectedLinkMetadata>> {
            let id = url_id(url)?;
            let path = self.library.article_path(&id);
            let mut bytes = fs::read(&path)?;
            bytes.extend_from_slice(b"\nConcurrent user's text.\n");
            fs::write(path, bytes)?;
            Ok(Some(DetectedLinkMetadata {
                category: LinkCategory::Book,
                title: None,
                image_url: None,
                isbn: None,
            }))
        }
        fn artwork(&self, _: &str) -> Result<ImageBytes> {
            unreachable!()
        }
    }
    #[test]
    fn concurrent_edits_are_preserved_and_stale_backups_are_removed() {
        let dir = TempDir::new().unwrap();
        let backup = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = import_link(&library, "https://example.com/book").unwrap();
        let report = enrich_existing_categories_with_fetcher(
            options(dir.path(), Some(backup.path()), true),
            Arc::new(ConcurrentFetcher {
                library: LibraryRoot::new(dir.path()).unwrap(),
            }),
        )
        .unwrap();
        assert_eq!((report.changed, report.unchanged, report.errors), (0, 1, 0));
        assert!(!backup.path().join(format!("{}.md", saved.id)).exists());
        assert!(fs::read_to_string(library.article_path(&saved.id))
            .unwrap()
            .ends_with("Concurrent user's text.\n"));
        assert_eq!(
            fs::read_dir(library.assets_dir(&saved.id)).unwrap().count(),
            0
        );
    }

    #[test]
    fn reviewed_id_subset_filters_before_guards_fetches_and_backups() {
        let dir = TempDir::new().unwrap();
        let backup = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let book = import_link(&library, "https://example.com/book").unwrap();
        let music = import_link(&library, "https://example.com/music").unwrap();
        let untouched = fs::read(library.article_path(&music.id)).unwrap();
        let fetcher = Arc::new(FixtureFetcher::new(false));
        let mut args = options(dir.path(), Some(backup.path()), true);
        args.reading_ids = Some(vec![book.id.clone()]);
        args.expected_count = Some(1);
        args.expected_digest = Some(sha256_hex(format!("{}\n", book.id).as_bytes()));
        let report = enrich_existing_categories_with_fetcher(args, fetcher.clone()).unwrap();
        assert_eq!((report.planned, report.changed), (1, 1));
        assert_eq!(fetcher.calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            fs::read(library.article_path(&music.id)).unwrap(),
            untouched
        );
        assert!(!backup.path().join(format!("{}.md", music.id)).exists());
    }

    #[test]
    fn invalid_duplicate_or_ineligible_id_selections_fail_before_any_fetch() {
        let dir = TempDir::new().unwrap();
        let backup = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let book = import_link(&library, "https://example.com/book").unwrap();
        let full = import_link(&library, "https://example.com/full").unwrap();
        let text = fs::read_to_string(library.article_path(&full.id))
            .unwrap()
            .replace("lightweight: true", "lightweight: false");
        fs::write(library.article_path(&full.id), text).unwrap();
        let fetcher = Arc::new(FixtureFetcher::new(false));
        for selection in [
            vec![],
            vec!["bad-id".into()],
            vec![book.id.clone(), book.id.clone()],
            vec!["0".repeat(64)],
            vec![full.id],
        ] {
            let mut args = options(dir.path(), Some(backup.path()), true);
            args.reading_ids = Some(selection);
            assert!(enrich_existing_categories_with_fetcher(args, fetcher.clone()).is_err());
        }
        assert_eq!(fetcher.calls.load(Ordering::Relaxed), 0);
        assert_eq!(fs::read_dir(backup.path()).unwrap().count(), 0);
    }
}
