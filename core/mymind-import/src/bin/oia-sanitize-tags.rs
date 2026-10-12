// SPDX-License-Identifier: MIT

use std::{collections::BTreeMap, path::PathBuf, process::ExitCode};

use anyhow::Result;
use clap::Parser;
use oia_core::{
    sanitation::{apply_tag_renames, plan_tag_renames},
    LibraryRoot,
};

/// Preview or apply explicitly reviewed Tag renames to an existing Óia library.
#[derive(Parser)]
#[command(
    version,
    about = "Preview or apply explicitly reviewed Tag renames in an Óia library"
)]
struct Arguments {
    #[arg(long)]
    library: PathBuf,
    /// JSON object mapping old Tag names to their reviewed replacement names.
    #[arg(long)]
    map: PathBuf,
    /// Write changes, with original files backed up outside the library.
    #[arg(long, requires_all = ["backup_dir", "expect_digest"])]
    apply: bool,
    #[arg(long, requires = "apply")]
    backup_dir: Option<PathBuf>,
    /// Digest emitted by the reviewed preview; guards both files and rename map.
    #[arg(long, requires = "apply")]
    expect_digest: Option<String>,
}

fn run(arguments: Arguments) -> Result<bool> {
    let names: BTreeMap<String, String> = serde_json::from_slice(&std::fs::read(arguments.map)?)?;
    let library = LibraryRoot::new(&arguments.library.canonicalize()?)?;
    let plan = plan_tag_renames(&library, &names)?;
    let report = if arguments.apply {
        apply_tag_renames(
            &library,
            plan,
            arguments.expect_digest.as_deref().unwrap(),
            arguments.backup_dir.as_deref().unwrap(),
        )?
    } else {
        plan.report
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(report.conflicts == 0)
}

fn main() -> ExitCode {
    match run(Arguments::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Tag sanitation failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_default_and_apply_requires_both_snapshot_and_backup() {
        let preview = Arguments::try_parse_from([
            "oia-sanitize-tags",
            "--library",
            "/tmp/library",
            "--map",
            "/tmp/map.json",
        ])
        .unwrap();
        assert!(!preview.apply);
        for extra in [
            vec!["--apply"],
            vec!["--apply", "--backup-dir", "/tmp/backup"],
            vec!["--apply", "--expect-digest", "digest"],
        ] {
            let mut args = vec![
                "oia-sanitize-tags",
                "--library",
                "/tmp/library",
                "--map",
                "/tmp/map.json",
            ];
            args.extend(extra);
            assert!(Arguments::try_parse_from(args).is_err());
        }
        assert!(
            Arguments::try_parse_from([
                "oia-sanitize-tags",
                "--library",
                "/tmp/library",
                "--map",
                "/tmp/map.json",
                "--apply",
                "--backup-dir",
                "/tmp/backup",
                "--expect-digest",
                "digest",
            ])
            .unwrap()
            .apply
        );
    }
}
