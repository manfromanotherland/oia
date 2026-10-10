// SPDX-License-Identifier: MIT

//! Explicit, recoverable enrichment of existing lightweight product links.

use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
};

use anyhow::{bail, Context, Result};
use oia_core::{
    fetch_product_metadata, parse_reading, sha256_hex, update_product_metadata_if_unchanged,
    url_id, LibraryRoot, ProductMetadata, ReadingKind,
};
use url::Url;

use crate::link_metadata::HostLimiter;

#[derive(Debug, Clone)]
pub struct EnrichExistingProductsOptions {
    pub library: PathBuf,
    /// Preview fetches evidence without modifying library files.
    pub write: bool,
    pub verbose: bool,
    pub workers: usize,
    pub expected_count: Option<usize>,
    /// SHA-256 of the sorted, newline-delimited eligible reading IDs.
    pub expected_digest: Option<String>,
    /// An empty directory outside the library; required when writing.
    pub backup_dir: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct EnrichExistingProductsReport {
    pub planned: usize,
    pub target_digest: String,
    pub fetched: usize,
    pub recognized: usize,
    pub priced: usize,
    pub upgraded: usize,
    pub unchanged: usize,
    pub fetch_failures: usize,
    pub errors: usize,
    pub backup_dir: Option<PathBuf>,
    write: bool,
    verbose_lines: Vec<String>,
}

impl EnrichExistingProductsReport {
    pub fn render(&self) -> String {
        let mut rendered = format!(
            "product metadata migration{}\nTarget links: {}\nTarget digest: {}\nProduct evidence: fetched {}, recognized {}, with prices {}, unavailable {}\n",
            if self.write { "" } else { " preview" },
            self.planned,
            self.target_digest,
            self.fetched,
            self.recognized,
            self.priced,
            self.fetch_failures,
        );
        if let Some(backup) = &self.backup_dir {
            rendered.push_str(&format!(
                "Original files and report: {}\n",
                backup.display()
            ));
        }
        for line in &self.verbose_lines {
            rendered.push_str(line);
            rendered.push('\n');
        }
        rendered.push_str(&format!(
            "\nSummary: target links {}, products {}, changed {}, unchanged {}, unavailable {}, errors {}\n",
            self.planned,
            self.recognized,
            self.upgraded,
            self.unchanged,
            self.fetch_failures,
            self.errors,
        ));
        if !self.write {
            rendered.push_str(
                "No library files were written. Re-run with --write and --backup-dir to apply.\n",
            );
        }
        rendered
    }
}

struct ProductTarget {
    id: String,
    url: String,
    original: Vec<u8>,
    fingerprint: String,
}

trait ProductMetadataFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Option<ProductMetadata>>;
}

struct HttpProductMetadataFetcher {
    hosts: HostLimiter,
}

impl ProductMetadataFetcher for HttpProductMetadataFetcher {
    fn fetch(&self, url: &str) -> Result<Option<ProductMetadata>> {
        let parsed = Url::parse(url)?;
        let host = parsed.host_str().context("product URL has no host")?;
        let _permit = self.hosts.acquire(host);
        // The core fetcher validates and pins public DNS for every request and
        // redirect, caps HTML at 2 MiB, and has bounded request timeouts.
        fetch_product_metadata(url).or_else(|_| fetch_product_metadata(url))
    }
}

pub fn enrich_existing_products(
    options: EnrichExistingProductsOptions,
) -> Result<EnrichExistingProductsReport> {
    enrich_existing_products_with_fetcher(
        options,
        Arc::new(HttpProductMetadataFetcher {
            hosts: HostLimiter::new(2),
        }),
    )
}

fn enrich_existing_products_with_fetcher(
    options: EnrichExistingProductsOptions,
    fetcher: Arc<dyn ProductMetadataFetcher>,
) -> Result<EnrichExistingProductsReport> {
    let library_path = options
        .library
        .canonicalize()
        .context("could not resolve the library folder")?;
    if !library_path.is_dir() {
        bail!("Óia library is not a directory");
    }
    let library = LibraryRoot::new(&library_path)?;
    let targets = product_targets(&library)?;
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
        if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
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
    let mut report = EnrichExistingProductsReport {
        planned: targets.len(),
        target_digest: digest,
        backup_dir: backup.clone(),
        write: options.write,
        ..Default::default()
    };
    let total = targets.len();
    let workers = options.workers.clamp(1, 32).min(total.max(1));
    let queue = Arc::new(Mutex::new(VecDeque::from(targets)));
    let (sender, receiver) = mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..workers {
            let queue = Arc::clone(&queue);
            let fetcher = Arc::clone(&fetcher);
            let sender = sender.clone();
            scope.spawn(move || loop {
                let Some(target) = queue
                    .lock()
                    .expect("product queue lock poisoned")
                    .pop_front()
                else {
                    break;
                };
                let result = fetcher.fetch(&target.url);
                if sender.send((target, result)).is_err() {
                    break;
                }
            });
        }
        drop(sender);
        for (index, (target, result)) in receiver.into_iter().enumerate() {
            match result {
                Ok(Some(product)) => {
                    report.fetched += 1;
                    report.recognized += 1;
                    report.priced +=
                        usize::from(product.price.is_some() && product.currency.is_some());
                    if options.verbose {
                        report.verbose_lines.push(format!(
                            "PRODUCT reading {}: {}{} {}",
                            target.id,
                            if product.price_is_from { "from " } else { "" },
                            product.currency.as_deref().unwrap_or("price unavailable"),
                            product.price.as_deref().unwrap_or(""),
                        ));
                    }
                    if let Some(backup) = backup.as_deref() {
                        match apply_product(&library, backup, &target, product) {
                            Ok(true) => report.upgraded += 1,
                            Ok(false) => {
                                report.unchanged += 1;
                                if options.verbose {
                                    report.verbose_lines.push(format!(
                                        "UNCHANGED reading {}: edited while evidence was fetched",
                                        target.id
                                    ));
                                }
                            }
                            Err(_) => {
                                report.errors += 1;
                                if options.verbose {
                                    report.verbose_lines.push(format!("ERROR reading {}: product metadata could not be safely written; original retained in backup", target.id));
                                }
                            }
                        }
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
                eprintln!("Product metadata: processed {processed}/{total}");
            }
        }
    });
    report.verbose_lines.sort();
    if let Some(backup) = backup {
        fs::write(backup.join("report.txt"), report.render())
            .context("could not save the product migration report")?;
    }
    Ok(report)
}

fn product_targets(library: &LibraryRoot) -> Result<Vec<ProductTarget>> {
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
            let article = entry.path().join("article.md");
            if fs::symlink_metadata(&article)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                continue;
            }
            let Ok(original) = fs::read(&article) else {
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
                || metadata.source_profile.is_some()
                || metadata.product.is_some()
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
            targets.push(ProductTarget {
                id: metadata.id,
                url,
                fingerprint: sha256_hex(&original),
                original,
            });
        }
    }
    targets.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(targets)
}

fn target_digest(targets: &[ProductTarget]) -> String {
    let mut joined = targets
        .iter()
        .map(|target| target.id.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    sha256_hex(joined.as_bytes())
}

pub(crate) fn prepare_backup_directory(
    library: &Path,
    requested: Option<&Path>,
) -> Result<PathBuf> {
    let library = library.canonicalize()?;
    let requested = requested.context("--backup-dir is required when writing metadata")?;
    if requested
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("backup path must not contain parent-directory components");
    }
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        std::env::current_dir()?.join(requested)
    };
    let mut ancestor = absolute.clone();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(
            ancestor
                .file_name()
                .context("invalid backup folder")?
                .to_os_string(),
        );
        ancestor = ancestor
            .parent()
            .context("invalid backup folder")?
            .to_path_buf();
    }
    let mut backup = ancestor.canonicalize()?;
    for component in suffix.into_iter().rev() {
        backup.push(component);
    }
    if backup.starts_with(&library) || library.starts_with(&backup) {
        bail!("backups must be outside the library folder and must not contain it");
    }
    fs::create_dir_all(&backup).context("could not create the backup directory")?;
    if fs::read_dir(&backup)?.next().is_some() {
        bail!("backup directory must be empty");
    }
    Ok(backup)
}

fn apply_product(
    library: &LibraryRoot,
    backup: &Path,
    target: &ProductTarget,
    product: ProductMetadata,
) -> Result<bool> {
    let path = backup.join(format!("{}.md", target.id));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&target.original)?;
    file.sync_all()?;
    drop(file);
    match update_product_metadata_if_unchanged(library, &target.id, product, &target.fingerprint) {
        Ok(false) => {
            fs::remove_file(path)?;
            Ok(false)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oia_core::{import_link, render_reading};
    use tempfile::TempDir;

    struct FixtureFetcher;
    impl ProductMetadataFetcher for FixtureFetcher {
        fn fetch(&self, url: &str) -> Result<Option<ProductMetadata>> {
            if url.contains("unavailable") {
                bail!("unavailable");
            }
            Ok(url.contains("product").then(|| ProductMetadata {
                version: 1,
                price: Some("229.00".into()),
                currency: Some("EUR".into()),
                price_is_from: false,
                captured_at: Some("2026-10-10T10:00:00Z".into()),
            }))
        }
    }

    fn options(
        library: &Path,
        write: bool,
        backup: Option<&Path>,
    ) -> EnrichExistingProductsOptions {
        EnrichExistingProductsOptions {
            library: library.to_path_buf(),
            write,
            verbose: true,
            workers: 2,
            expected_count: None,
            expected_digest: None,
            backup_dir: backup.map(Path::to_path_buf),
        }
    }

    #[test]
    fn preview_detects_products_without_writing_and_write_backs_up_original_files() {
        let library = TempDir::new().unwrap();
        let backups = TempDir::new().unwrap();
        let root = LibraryRoot::new(library.path()).unwrap();
        let product = "https://example.com/product";
        import_link(&root, product).unwrap();
        import_link(&root, "https://example.com/article").unwrap();
        import_link(&root, "https://example.com/unavailable").unwrap();
        let id = url_id(product).unwrap();
        let mut reading =
            parse_reading(&fs::read_to_string(root.article_path(&id)).unwrap()).unwrap();
        reading.metadata.tags = vec!["lighting".into()];
        reading.metadata.favorite = true;
        reading.metadata.preview_asset = Some("assets/existing.webp".into());
        reading.body = "Existing imported body\n".into();
        fs::write(root.article_path(&id), render_reading(&reading).unwrap()).unwrap();
        let original = fs::read(root.article_path(&id)).unwrap();
        fs::write(root.note_path(&id), "Keep the note").unwrap();
        let preview = enrich_existing_products_with_fetcher(
            options(library.path(), false, None),
            Arc::new(FixtureFetcher),
        )
        .unwrap();
        assert_eq!(
            (
                preview.planned,
                preview.recognized,
                preview.priced,
                preview.fetch_failures
            ),
            (3, 1, 1, 1)
        );
        assert_eq!(fs::read(root.article_path(&id)).unwrap(), original);
        let mut apply = options(library.path(), true, Some(backups.path()));
        apply.expected_count = Some(preview.planned);
        apply.expected_digest = Some(preview.target_digest);
        let report =
            enrich_existing_products_with_fetcher(apply, Arc::new(FixtureFetcher)).unwrap();
        assert_eq!(
            (report.upgraded, report.unchanged, report.errors),
            (1, 2, 0)
        );
        assert_eq!(
            fs::read(backups.path().join(format!("{id}.md"))).unwrap(),
            original
        );
        assert!(backups.path().join("report.txt").is_file());
        let updated = parse_reading(&fs::read_to_string(root.article_path(&id)).unwrap()).unwrap();
        assert_eq!(updated.body, reading.body);
        assert_eq!(updated.metadata.tags, vec!["lighting"]);
        assert!(updated.metadata.favorite);
        assert_eq!(
            updated.metadata.preview_asset,
            reading.metadata.preview_asset
        );
        assert_eq!(
            updated.metadata.product.unwrap().price.as_deref(),
            Some("229")
        );
        assert_eq!(
            fs::read_to_string(root.note_path(&id)).unwrap(),
            "Keep the note"
        );
        assert_eq!(product_targets(&root).unwrap().len(), 2);
    }

    #[test]
    fn target_selection_excludes_full_articles_social_posts_and_existing_products() {
        let library = TempDir::new().unwrap();
        let root = LibraryRoot::new(library.path()).unwrap();
        for category in ["eligible", "full", "social", "existing"] {
            let url = format!("https://example.com/{category}");
            import_link(&root, &url).unwrap();
            let path = root.article_path(&url_id(&url).unwrap());
            let mut reading = parse_reading(&fs::read_to_string(&path).unwrap()).unwrap();
            match category {
                "full" => reading.metadata.lightweight = false,
                "social" => {
                    reading.metadata.source_profile = Some(oia_core::SourceProfile {
                        version: 1,
                        source_type: "social_post".into(),
                        provider: "x".into(),
                        source_id: "123".into(),
                        author_handle: "example".into(),
                        published_at: None,
                        avatar_asset: None,
                        attachments: vec![],
                    })
                }
                "existing" => {
                    reading.metadata.product = Some(ProductMetadata {
                        version: 1,
                        price: None,
                        currency: None,
                        price_is_from: false,
                        captured_at: None,
                    })
                }
                _ => {}
            }
            fs::write(path, render_reading(&reading).unwrap()).unwrap();
        }
        let targets = product_targets(&root).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].url, "https://example.com/eligible");
    }

    struct EditingFetcher {
        library: PathBuf,
    }
    impl ProductMetadataFetcher for EditingFetcher {
        fn fetch(&self, url: &str) -> Result<Option<ProductMetadata>> {
            let root = LibraryRoot::new(&self.library)?;
            let path = root.article_path(&url_id(url)?);
            let mut reading = parse_reading(&fs::read_to_string(&path)?)?;
            reading.metadata.title = "Concurrent edit".into();
            fs::write(path, render_reading(&reading)?)?;
            FixtureFetcher.fetch(url)
        }
    }

    #[test]
    fn changed_files_are_retained_without_stale_backups() {
        let library = TempDir::new().unwrap();
        let backups = TempDir::new().unwrap();
        let root = LibraryRoot::new(library.path()).unwrap();
        let url = "https://example.com/product";
        import_link(&root, url).unwrap();
        let report = enrich_existing_products_with_fetcher(
            options(library.path(), true, Some(backups.path())),
            Arc::new(EditingFetcher {
                library: library.path().to_path_buf(),
            }),
        )
        .unwrap();
        assert_eq!(
            (report.upgraded, report.unchanged, report.errors),
            (0, 1, 0)
        );
        let reading =
            parse_reading(&fs::read_to_string(root.article_path(&url_id(url).unwrap())).unwrap())
                .unwrap();
        assert_eq!(reading.metadata.title, "Concurrent edit");
        assert!(reading.metadata.product.is_none());
        assert_eq!(fs::read_dir(backups.path()).unwrap().count(), 1);
    }

    #[test]
    fn backup_directory_inside_library_is_rejected_before_creating_it() {
        let library = TempDir::new().unwrap();
        let backup = library.path().join("backup");
        assert!(prepare_backup_directory(library.path(), Some(&backup)).is_err());
        assert!(!backup.exists());
    }

    #[test]
    fn changed_target_guard_runs_before_fetching_or_creating_backups() {
        let library = TempDir::new().unwrap();
        let root = LibraryRoot::new(library.path()).unwrap();
        import_link(&root, "https://example.com/product").unwrap();
        let mut apply = options(library.path(), true, None);
        apply.expected_count = Some(2);
        let error =
            enrich_existing_products_with_fetcher(apply, Arc::new(FixtureFetcher)).unwrap_err();
        assert!(error.to_string().contains("target snapshot changed"));
    }
}
