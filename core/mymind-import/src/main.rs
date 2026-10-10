// SPDX-License-Identifier: MIT

use std::{path::PathBuf, process::ExitCode};

use clap::{ArgGroup, Parser};
use oia_import_mymind::{
    enrich_existing_categories, enrich_existing_links, enrich_existing_products, run,
    EnrichExistingCategoriesOptions, EnrichExistingOptions, EnrichExistingProductsOptions,
    RunOptions,
};

/// Import a mymind export folder into a Óia library.
///
/// The command previews by default. Pass --write only after reviewing the
/// planned cards and warnings.
#[derive(Debug, Parser)]
#[command(version, about, verbatim_doc_comment)]
#[command(group(ArgGroup::new("migration").args(["enrich_existing_links", "enrich_existing_products", "enrich_existing_categories"]).multiple(false)))]
struct Arguments {
    /// Folder created by mymind's "Export my mind" action.
    #[arg(
        long,
        value_name = "FOLDER",
        required_unless_present_any = ["enrich_existing_links", "enrich_existing_products", "enrich_existing_categories"],
        conflicts_with = "migration"
    )]
    export: Option<PathBuf>,

    /// Existing Óia library folder to receive the imported cards.
    #[arg(long, value_name = "FOLDER")]
    library: PathBuf,

    /// Write the planned cards. Without this flag, no library files change.
    #[arg(long)]
    write: bool,

    /// Show one line per reading, using opaque card and Óia IDs only.
    #[arg(long)]
    verbose: bool,

    /// Enrich lightweight links already in the library instead of importing an export.
    #[arg(long, conflicts_with = "offline")]
    enrich_existing_links: bool,

    /// Identify product pages and save price snapshots for existing lightweight links.
    #[arg(long, conflicts_with = "offline")]
    enrich_existing_products: bool,

    /// Identify books, music, and movies among existing lightweight links.
    #[arg(long, conflicts_with = "offline")]
    enrich_existing_categories: bool,

    /// Categorize only reviewed reading IDs, one 64-character hexadecimal ID per line.
    #[arg(long, requires = "enrich_existing_categories", conflicts_with_all = ["enrich_existing_products", "enrich_existing_links", "export", "offline"], value_name = "FILE")]
    only_ids_file: Option<PathBuf>,

    /// Empty folder outside the library for original files and a migration report.
    #[arg(long, requires = "migration", value_name = "FOLDER")]
    backup_dir: Option<PathBuf>,

    /// Keep imported links URL-only; do not fetch website metadata or assets.
    #[arg(long)]
    offline: bool,

    /// Maximum concurrent website fetches (each host is capped at two).
    #[arg(long, default_value_t = 24)]
    workers: usize,

    /// Abort unless the migration target contains exactly this many readings.
    #[arg(long, requires = "migration")]
    expect_count: Option<usize>,

    /// Abort unless the migration target has this SHA-256 digest.
    #[arg(long, requires = "migration", value_name = "SHA256")]
    expect_digest: Option<String>,
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    if arguments.enrich_existing_categories {
        let reading_ids = match arguments
            .only_ids_file
            .as_ref()
            .map(std::fs::read_to_string)
            .transpose()
        {
            Ok(contents) => contents.map(|text| text.lines().map(str::to_owned).collect()),
            Err(_) => {
                eprintln!(
                    "link category migration failed: could not read the category ID selection file"
                );
                return ExitCode::FAILURE;
            }
        };
        match enrich_existing_categories(EnrichExistingCategoriesOptions {
            library: arguments.library,
            write: arguments.write,
            verbose: arguments.verbose,
            workers: arguments.workers,
            expected_count: arguments.expect_count,
            expected_digest: arguments.expect_digest,
            backup_dir: arguments.backup_dir,
            reading_ids,
        }) {
            Ok(report) => {
                print!("{}", report.render());
                if report.errors == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                eprintln!("link category migration failed: {error:#}");
                ExitCode::FAILURE
            }
        }
    } else if arguments.enrich_existing_products {
        match enrich_existing_products(EnrichExistingProductsOptions {
            library: arguments.library,
            write: arguments.write,
            verbose: arguments.verbose,
            workers: arguments.workers,
            expected_count: arguments.expect_count,
            expected_digest: arguments.expect_digest,
            backup_dir: arguments.backup_dir,
        }) {
            Ok(report) => {
                print!("{}", report.render());
                if report.errors == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                eprintln!("product metadata migration failed: {error:#}");
                ExitCode::FAILURE
            }
        }
    } else if arguments.enrich_existing_links {
        match enrich_existing_links(EnrichExistingOptions {
            library: arguments.library,
            write: arguments.write,
            verbose: arguments.verbose,
            workers: arguments.workers,
            expected_count: arguments.expect_count,
            expected_digest: arguments.expect_digest,
        }) {
            Ok(report) => {
                print!("{}", report.render());
                if report.errors == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                eprintln!("link metadata migration failed: {error:#}");
                ExitCode::FAILURE
            }
        }
    } else {
        match run(RunOptions {
            export: arguments
                .export
                .expect("clap requires an export outside migration mode"),
            library: arguments.library,
            write: arguments.write,
            verbose: arguments.verbose,
            enrich_links: !arguments.offline,
            workers: arguments.workers,
        }) {
            Ok(report) => {
                print!("{}", report.render());
                if report.errors == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                eprintln!("mymind import failed: {error:#}");
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_migration_is_an_alternative_to_an_export() {
        let arguments = Arguments::try_parse_from([
            "oia-import-mymind",
            "--library",
            "/tmp/library",
            "--enrich-existing-products",
            "--expect-count",
            "3",
            "--expect-digest",
            "abc",
        ])
        .unwrap();
        assert!(arguments.enrich_existing_products);
        assert!(arguments.export.is_none());
        assert_eq!(arguments.expect_count, Some(3));
    }

    #[test]
    fn migration_modes_and_export_cannot_be_combined() {
        for extra in ["--enrich-existing-links", "--offline"] {
            assert!(Arguments::try_parse_from([
                "oia-import-mymind",
                "--library",
                "/tmp/library",
                "--enrich-existing-products",
                extra,
            ])
            .is_err());
        }
        assert!(Arguments::try_parse_from([
            "oia-import-mymind",
            "--library",
            "/tmp/library",
            "--enrich-existing-products",
            "--export",
            "/tmp/export",
        ])
        .is_err());
    }

    #[test]
    fn category_migration_accepts_snapshot_guards_and_backup_without_export() {
        let arguments = Arguments::try_parse_from([
            "oia-import-mymind",
            "--library",
            "/tmp/library",
            "--enrich-existing-categories",
            "--write",
            "--backup-dir",
            "/tmp/backup",
            "--expect-count",
            "12",
        ])
        .unwrap();
        assert!(arguments.enrich_existing_categories);
        assert!(arguments.write);
        assert_eq!(arguments.expect_count, Some(12));
        assert!(arguments.export.is_none());
        let scoped = Arguments::try_parse_from([
            "oia-import-mymind",
            "--library",
            "/tmp/library",
            "--enrich-existing-categories",
            "--only-ids-file",
            "/tmp/reviewed-ids.txt",
        ])
        .unwrap();
        assert_eq!(
            scoped.only_ids_file.as_deref(),
            Some(std::path::Path::new("/tmp/reviewed-ids.txt"))
        );
        assert!(Arguments::try_parse_from([
            "oia-import-mymind",
            "--library",
            "/tmp/library",
            "--enrich-existing-products",
            "--only-ids-file",
            "/tmp/reviewed-ids.txt"
        ])
        .is_err());
        for extra in [
            "--enrich-existing-products",
            "--enrich-existing-links",
            "--offline",
        ] {
            assert!(Arguments::try_parse_from([
                "oia-import-mymind",
                "--library",
                "/tmp/library",
                "--enrich-existing-categories",
                extra
            ])
            .is_err());
        }
    }
}
