// SPDX-License-Identifier: MIT

//! Explicit, recoverable Tag renames over file-authoritative library metadata.

use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use rustix::fs::{AtFlags, Mode, OFlags};
use serde::Serialize;
use serde_yaml::{Mapping, Value};

use crate::{tags::tag_key, LibraryRoot};

#[derive(Debug, Default, Serialize)]
pub struct SanitationReport {
    pub scanned: usize,
    pub planned: usize,
    pub changed: usize,
    pub conflicts: usize,
    pub renamed_labels: usize,
    pub merged_duplicates: usize,
    pub digest: String,
    pub renames: BTreeMap<String, usize>,
    pub changed_ids: Vec<String>,
    pub conflict_ids: Vec<String>,
    pub files: BTreeMap<String, FileSnapshot>,
}

#[derive(Debug, Serialize)]
pub struct FileSnapshot {
    pub original_sha256: String,
    pub changed_sha256: String,
}

struct Target {
    id: String,
    original: Vec<u8>,
    replacement: String,
    sha256: String,
}

pub struct SanitationPlan {
    library: PathBuf,
    targets: Vec<Target>,
    pub report: SanitationReport,
}

/// Preview only the explicitly supplied old-name -> new-name mappings. Keys
/// use the same case-folded identity as normal Tag editing. No punctuation,
/// spelling, plural, or compound-word heuristic is applied.
pub fn plan_tag_renames(
    library: &LibraryRoot,
    names: &BTreeMap<String, String>,
) -> Result<SanitationPlan> {
    let names = validate_names(names)?;
    let mut directories = crate::scanner::reading_directories(library)?;
    directories.sort();
    let mut inventory = serde_json::to_vec(&names)?;
    let mut targets = Vec::new();
    let mut report = SanitationReport::default();
    for directory in directories {
        let id = directory
            .file_name()
            .and_then(|name| name.to_str())
            .context("invalid reading folder name")?;
        match directory.join("article.md").symlink_metadata() {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).with_context(|| format!("could not inspect {id}")),
        }
        if library.reading_dir(id) != directory {
            bail!("reading folder has an unexpected prefix: {id}");
        }
        let dir = crate::notes::open_reading_dir_components(library, id)?;
        let (original, _) = read_article(&dir)?;
        let text = std::str::from_utf8(&original)?;
        if crate::parse_reading(text)?.metadata.id != id {
            bail!("reading identity does not match its folder: {id}");
        }
        let sha256 = crate::sha256_hex(&original);
        inventory.extend_from_slice(format!("\n{id}\t{sha256}").as_bytes());
        report.scanned += 1;
        if let Some(replacement) = patch_tags(text, &names, &mut report)? {
            targets.push(Target {
                id: id.into(),
                original,
                replacement,
                sha256,
            });
        }
    }
    report.planned = targets.len();
    report.digest = crate::sha256_hex(&inventory);
    Ok(SanitationPlan {
        library: library.path().canonicalize()?,
        targets,
        report,
    })
}

/// Apply a freshly generated preview whose inventory/map digest still matches
/// the reviewed preview. Original bytes are backed up before every successful
/// atomic write. Files changed or deleted since planning are skipped.
pub fn apply_tag_renames(
    library: &LibraryRoot,
    mut plan: SanitationPlan,
    expected_digest: &str,
    backup: &Path,
) -> Result<SanitationReport> {
    if expected_digest != plan.report.digest || plan.library != library.path().canonicalize()? {
        bail!("Tag rename snapshot changed; run a new preview before applying");
    }
    let backup = prepare_backup(library, backup)?;
    write_report(&backup, &plan.report)?;
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(backup.join("journal.jsonl"))?;
    for target in &plan.targets {
        if apply_target(library, target, &backup, || {})? {
            plan.report.changed += 1;
            plan.report.changed_ids.push(target.id.clone());
            plan.report.files.insert(
                target.id.clone(),
                FileSnapshot {
                    original_sha256: target.sha256.clone(),
                    changed_sha256: crate::sha256_hex(target.replacement.as_bytes()),
                },
            );
        } else {
            plan.report.conflicts += 1;
            plan.report.conflict_ids.push(target.id.clone());
        }
        let record = serde_json::json!({
            "id": target.id,
            "changed": plan.report.files.contains_key(&target.id),
            "original_sha256": target.sha256,
            "changed_sha256": crate::sha256_hex(target.replacement.as_bytes()),
        });
        serde_json::to_writer(&mut journal, &record)?;
        journal.write_all(b"\n")?;
        journal.sync_all()?;
    }
    write_report(&backup, &plan.report)?;
    Ok(plan.report)
}

fn validate_names(names: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
    let mut normalized = BTreeMap::new();
    for (old, new) in names {
        let key = tag_key(old);
        if key.is_empty() || new.trim().is_empty() || new != new.trim() {
            bail!("rename names must be nonempty and replacements must be trimmed");
        }
        if new.chars().any(char::is_control) {
            bail!("Tag replacement contains a control character: {new:?}");
        }
        if new.chars().count() > old.chars().count() {
            bail!("Tag replacement must not lengthen an existing label: {old:?}");
        }
        if let Some(previous) = normalized.insert(key.clone(), new.clone()) {
            if previous != *new {
                bail!("conflicting case-insensitive mappings for {old:?}");
            }
        }
    }
    for (old, new) in &normalized {
        if let Some(next) = normalized.get(&tag_key(new)) {
            if next != new {
                bail!("chained Tag rename from {old:?} through {new:?} is not idempotent");
            }
        }
    }
    Ok(normalized)
}

fn rename_list(
    value: &mut Value,
    names: &BTreeMap<String, String>,
    exclusions: bool,
    report: &mut SanitationReport,
) -> Result<()> {
    if value.is_null() {
        return Ok(());
    }
    let sequence = value
        .as_sequence_mut()
        .context("Tag labels are not an array")?;
    let mut seen = HashSet::new();
    let mut updated = Vec::new();
    for item in sequence.iter() {
        let original = item.as_str().context("Tag label is not a string")?;
        let old_key = tag_key(original);
        let label = match names.get(&old_key) {
            Some(label) if exclusions => tag_key(label),
            Some(label) => label.clone(),
            None => original.to_owned(),
        };
        if label != original {
            report.renamed_labels += 1;
            *report.renames.entry(old_key).or_default() += 1;
        }
        if seen.insert(tag_key(&label)) {
            updated.push(Value::String(label));
        } else {
            report.merged_duplicates += 1;
        }
    }
    *sequence = updated;
    Ok(())
}

fn patch_tags(
    original: &str,
    names: &BTreeMap<String, String>,
    report: &mut SanitationReport,
) -> Result<Option<String>> {
    let rest = original
        .strip_prefix("---\n")
        .context("missing frontmatter")?;
    let close = rest.find("\n---\n").context("missing frontmatter close")?;
    let header = &rest[..close];
    let mut metadata: Mapping = serde_yaml::from_str(header)?;
    let mut replacements = BTreeMap::new();
    for key in ["tags", "machine_tags", "excluded_machine_tags"] {
        let Some(value) = metadata.get_mut(Value::String(key.into())) else {
            continue;
        };
        let before = value.clone();
        if key == "machine_tags" {
            if !value.is_null() {
                for source in value
                    .as_sequence_mut()
                    .context("machine Tags are not an array")?
                {
                    let source = source
                        .as_mapping_mut()
                        .context("invalid machine Tag source")?;
                    if let Some(tags) = source.get_mut(Value::String("tags".into())) {
                        rename_list(tags, names, false, report)?;
                    }
                }
            }
        } else {
            rename_list(value, names, key == "excluded_machine_tags", report)?;
        }
        if *value != before {
            let mut block = Mapping::new();
            block.insert(Value::String(key.into()), value.clone());
            replacements.insert(key.to_string(), serde_yaml::to_string(&block)?);
        }
    }
    if replacements.is_empty() {
        return Ok(None);
    }
    // Replace only changed Tag fields. All other header lines, unknown fields,
    // comments outside Tag blocks, fences, and raw body bytes remain intact.
    let mut retained = String::new();
    let mut skipping = false;
    let mut replaced = HashSet::new();
    for line in header.split_inclusive('\n') {
        let comment = line.trim_start().starts_with('#');
        let blank = line.trim().is_empty();
        let sequence_item = line.starts_with("- ") || line.trim() == "-";
        let top_level = !line.starts_with([' ', '\t']) && !sequence_item && !comment && !blank;
        if top_level {
            let key = line
                .split_once(':')
                .map(|(key, _)| key.trim().trim_matches(['\'', '"']));
            skipping = key.is_some_and(|key| replacements.contains_key(key));
            if let Some(block) = key.and_then(|key| replacements.get(key)) {
                retained.push_str(block);
                replaced.insert(key.unwrap().to_owned());
            }
        }
        if !skipping || comment || blank {
            retained.push_str(line);
        }
    }
    if replaced.len() != replacements.len() {
        bail!("unsupported YAML layout in Tag fields; original file left unchanged");
    }
    if !retained.ends_with('\n') {
        retained.push('\n');
    }
    let replacement = format!("---\n{retained}{}", &rest[close + 1..]);
    // Reject unusual YAML aliases/layouts rather than changing unrelated data.
    let new_rest = replacement.strip_prefix("---\n").unwrap();
    let new_close = new_rest
        .find("\n---\n")
        .context("invalid patched frontmatter")?;
    let patched: Mapping = serde_yaml::from_str(&new_rest[..new_close])?;
    if patched != metadata {
        bail!("Tag patch would alter unrelated frontmatter; original file left unchanged");
    }
    crate::parse_reading(&replacement)?;
    Ok(Some(replacement))
}

fn read_article(dir: &rustix::fd::OwnedFd) -> Result<(Vec<u8>, fs::Permissions)> {
    let fd = rustix::fs::openat(
        dir,
        "article.md",
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let mut file = fs::File::from(fd);
    let stat = file.metadata()?;
    if !stat.is_file() {
        bail!("article is not a regular file");
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((bytes, stat.permissions()))
}

fn apply_target(
    library: &LibraryRoot,
    target: &Target,
    backup: &Path,
    before_commit: impl FnOnce(),
) -> Result<bool> {
    let lock = crate::locking::lock_reading(library, &target.id)?;
    let dir = match crate::notes::open_reading_dir_components(library, &target.id) {
        Ok(dir) => dir,
        Err(_) => return Ok(false),
    };
    let (current, permissions) = match read_article(&dir) {
        Ok(article) => article,
        Err(_) => return Ok(false),
    };
    if crate::sha256_hex(&current) != target.sha256 {
        return Ok(false);
    }
    let backup_file = backup.join(format!("{}.md", target.id));
    let mut original = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup_file)?;
    original.write_all(&target.original)?;
    original.sync_all()?;
    drop(original);
    fs::File::open(backup)?.sync_all()?;
    let temp_name = format!(".tags.{}.tmp", crate::new_id());
    let result = (|| -> Result<bool> {
        lock.ensure_protects(library, &target.id)?;
        let fd = rustix::fs::openat(
            &dir,
            temp_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )?;
        let mut file = fs::File::from(fd);
        file.set_permissions(permissions)?;
        file.write_all(target.replacement.as_bytes())?;
        file.sync_all()?;
        drop(file);
        before_commit();
        let Ok(current_dir) = crate::notes::open_reading_dir_components(library, &target.id) else {
            return Ok(false);
        };
        let expected_stat = rustix::fs::fstat(&dir)?;
        let current_stat = rustix::fs::fstat(&current_dir)?;
        if expected_stat.st_dev != current_stat.st_dev
            || expected_stat.st_ino != current_stat.st_ino
            || !read_article(&dir)
                .is_ok_and(|(bytes, _)| crate::sha256_hex(&bytes) == target.sha256)
        {
            return Ok(false);
        }
        rustix::fs::renameat(&dir, temp_name.as_str(), &dir, "article.md")?;
        rustix::fs::fsync(&dir)?;
        Ok(true)
    })();
    let _ = rustix::fs::unlinkat(&dir, temp_name.as_str(), AtFlags::empty());
    if result.as_ref().is_ok_and(|changed| !changed) {
        fs::remove_file(backup_file)?;
    }
    result
}

fn prepare_backup(library: &LibraryRoot, backup: &Path) -> Result<PathBuf> {
    let library = library.path().canonicalize()?;
    let mut ancestor = backup.to_path_buf();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(
            ancestor
                .file_name()
                .context("invalid backup folder")?
                .to_owned(),
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
        bail!("backup folder must be outside the library and must not contain it");
    }
    fs::create_dir_all(&backup)?;
    if fs::read_dir(&backup)?.next().is_some() {
        bail!("backup folder must be empty");
    }
    Ok(backup)
}

fn write_report(backup: &Path, report: &SanitationReport) -> Result<()> {
    let mut staged = tempfile::NamedTempFile::new_in(backup)?;
    serde_json::to_writer_pretty(&mut staged, report)?;
    staged.write_all(b"\n")?;
    staged.as_file().sync_all()?;
    staged
        .persist(backup.join("manifest.json"))
        .map_err(|error| error.error)?;
    fs::File::open(backup)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests;
