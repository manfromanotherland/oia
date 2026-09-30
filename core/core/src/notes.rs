// SPDX-License-Identifier: MIT

//! Per-reading personal notes, stored as plain Markdown.
//!
//! A note lives in `note.md` inside the reading's folder, beside `article.md`
//! and `highlights.md`. It is deliberately separate from the captured body so
//! editing a personal note never changes the saved source or its content hash.

use std::{fs, io::Read as _, io::Write as _};

use anyhow::{bail, Context, Result};
use rustix::fs::{AtFlags, Mode, OFlags};
use rustix::io::Errno;

use crate::{
    locking::{lock_reading, ReadingLock},
    LibraryRoot,
};

/// Read the personal note attached to `reading_id`.
///
/// A reading with no `note.md` returns `None`. The article file is validated
/// first so a malformed id or divergent synced folder can never turn this API
/// into an arbitrary sidecar-file reader.
pub fn get_note(library: &LibraryRoot, reading_id: &str) -> Result<Option<String>> {
    validate_reading_id(reading_id)?;
    let lock = lock_reading(library, reading_id)?;
    lock.ensure_protects(library, reading_id)?;
    let reading_dir = open_reading_dir(library, reading_id)?;
    let note_fd = match rustix::fs::openat(
        &reading_dir,
        "note.md",
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("could not open note for {reading_id}"))
        }
    };

    let mut file = fs::File::from(note_fd);
    if !file.metadata()?.is_file() {
        bail!("note is not a regular file for {reading_id}");
    }
    let mut markdown = String::new();
    file.read_to_string(&mut markdown)
        .with_context(|| format!("could not read note for {reading_id}"))?;
    Ok(Some(markdown))
}

/// Replace the personal Markdown note attached to `reading_id`.
///
/// Whitespace-only Markdown clears the note by removing `note.md`. Non-empty
/// Markdown is preserved byte-for-byte and written atomically through a sibling
/// temporary file, so an external sync process sees either the old note or the
/// complete new one, never a partially written document.
pub fn set_note(library: &LibraryRoot, reading_id: &str, markdown: &str) -> Result<()> {
    validate_reading_id(reading_id)?;
    let lock = lock_reading(library, reading_id)?;
    lock.ensure_protects(library, reading_id)?;
    let reading_dir = open_reading_dir(library, reading_id)?;

    write_note_to_dir(&reading_dir, reading_id, markdown)
}

/// Write a note while the caller holds this reading's lock, including before
/// `article.md` has been committed for a brand-new import.
///
/// Unlike [`set_note`], this helper deliberately does not require an existing
/// article. It is crate-private so only save orchestration that has already
/// derived a safe id, created the reading folder, and acquired its lock can use
/// the pre-commit behavior. The public API retains its full article/id check.
pub(crate) fn set_note_under_lock(
    library: &LibraryRoot,
    reading_id: &str,
    markdown: &str,
    lock: &ReadingLock,
) -> Result<()> {
    validate_reading_id(reading_id)?;
    lock.ensure_protects(library, reading_id)?;
    let reading_dir = open_reading_dir_components(library, reading_id)?;

    write_note_to_dir(&reading_dir, reading_id, markdown)
}

fn write_note_to_dir(
    reading_dir: &rustix::fd::OwnedFd,
    reading_id: &str,
    markdown: &str,
) -> Result<()> {
    if markdown.trim().is_empty() {
        match rustix::fs::unlinkat(reading_dir, "note.md", AtFlags::empty()) {
            Ok(()) => rustix::fs::fsync(reading_dir)?,
            Err(Errno::NOENT) => {}
            Err(error) => {
                return Err(error).with_context(|| format!("could not clear note for {reading_id}"))
            }
        }
        return Ok(());
    }

    // A unique, exclusively created sibling prevents concurrent writers from
    // sharing an inode and prevents a pre-planted temp symlink from being
    // followed. The directory descriptor pins every operation to the validated
    // reading folder even if a sync process renames paths around it.
    let temp_name = format!(".note.{}.tmp", crate::new_id());
    let write_result = (|| -> Result<()> {
        let temp_fd = rustix::fs::openat(
            reading_dir,
            temp_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH | Mode::WOTH,
        )?;
        let mut file = fs::File::from(temp_fd);
        file.write_all(markdown.as_bytes())?;
        file.sync_all()?;
        drop(file);
        rustix::fs::renameat(reading_dir, temp_name.as_str(), reading_dir, "note.md")?;
        rustix::fs::fsync(reading_dir)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = rustix::fs::unlinkat(reading_dir, temp_name.as_str(), AtFlags::empty());
    }
    write_result.with_context(|| format!("could not save note for {reading_id}"))
}

/// Open and pin the requested reading folder one component at a time, rejecting
/// symlinks below the library root. Keeping this descriptor alive across the
/// note operation closes the usual canonicalize-then-use race with sync tools.
fn open_reading_dir(library: &LibraryRoot, reading_id: &str) -> Result<rustix::fd::OwnedFd> {
    validate_reading_id(reading_id)?;

    let reading_dir = open_reading_dir_components(library, reading_id)?;

    let article_fd = rustix::fs::openat(
        &reading_dir,
        "article.md",
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .with_context(|| format!("reading not found: {reading_id}"))?;
    let mut article = fs::File::from(article_fd);
    if !article.metadata()?.is_file() {
        bail!("article is not a regular file for {reading_id}");
    }
    let mut content = String::new();
    article
        .read_to_string(&mut content)
        .with_context(|| format!("cannot read article for {reading_id}"))?;
    let metadata = crate::parse_reading(&content)
        .with_context(|| format!("cannot read frontmatter for {reading_id}"))?
        .metadata;
    if metadata.id != reading_id {
        bail!(
            "reading id mismatch: requested {reading_id}, frontmatter contains {}",
            metadata.id
        );
    }

    Ok(reading_dir)
}

/// Open and pin a reading directory without requiring its article commit
/// marker. Callers must validate the article separately unless they are in the
/// narrow pre-commit import path.
fn open_reading_dir_components(
    library: &LibraryRoot,
    reading_id: &str,
) -> Result<rustix::fd::OwnedFd> {
    validate_reading_id(reading_id)?;

    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    // The chosen library root itself may intentionally be a symlink, so follow
    // that one path once. Every library-owned child is opened with NOFOLLOW.
    let root = rustix::fs::open(
        library.path(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let articles = rustix::fs::openat(&root, "articles", directory_flags, Mode::empty())?;
    let prefix = reading_id.get(..2).unwrap_or(reading_id);
    let bucket = rustix::fs::openat(&articles, prefix, directory_flags, Mode::empty())?;
    let reading_dir = rustix::fs::openat(&bucket, reading_id, directory_flags, Mode::empty())
        .with_context(|| format!("reading not found: {reading_id}"))?;

    Ok(reading_dir)
}

fn validate_reading_id(reading_id: &str) -> Result<()> {
    if reading_id.is_empty() || !reading_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("invalid reading id: {reading_id:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::{new_id, write_reading, LibraryRoot, Metadata};

    fn make_library(dir: &TempDir) -> LibraryRoot {
        fs::create_dir_all(dir.path().join("articles")).unwrap();
        LibraryRoot::new(dir.path()).unwrap()
    }

    fn metadata(id: &str) -> Metadata {
        Metadata {
            format_version: 1,
            id: id.to_string(),
            kind: Default::default(),
            lightweight: false,
            url: "https://example.com/article".to_string(),
            media_url: None,
            preview_asset: None,
            favicon_asset: None,
            theme_color: None,
            canonical_url: "https://example.com/article".to_string(),
            title: "Test article".to_string(),
            author: None,
            site: Some("example.com".to_string()),
            source_profile: None,
            saved_at: "2026-06-13T15:00:00Z".to_string(),
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
        }
    }

    fn saved_reading() -> (TempDir, LibraryRoot, String) {
        let dir = TempDir::new().unwrap();
        let library = make_library(&dir);
        let id = new_id();
        write_reading(&library, metadata(&id), "Captured body".into()).unwrap();
        (dir, library, id)
    }

    #[test]
    fn missing_note_returns_none() {
        let (_dir, library, id) = saved_reading();
        assert_eq!(get_note(&library, &id).unwrap(), None);
    }

    #[test]
    fn markdown_round_trips_without_rewriting() {
        let (_dir, library, id) = saved_reading();
        let markdown = "## Remember this\n\n- first\n- [source](https://example.com)";

        set_note(&library, &id, markdown).unwrap();

        assert_eq!(get_note(&library, &id).unwrap().as_deref(), Some(markdown));
        assert_eq!(
            fs::read_to_string(library.note_path(&id)).unwrap(),
            markdown
        );
    }

    #[test]
    fn replacing_note_overwrites_the_complete_document() {
        let (_dir, library, id) = saved_reading();
        set_note(&library, &id, "old note").unwrap();

        set_note(&library, &id, "**new** note").unwrap();

        assert_eq!(
            get_note(&library, &id).unwrap().as_deref(),
            Some("**new** note")
        );
        assert!(!fs::read_dir(library.reading_dir(&id))
            .unwrap()
            .any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            }));
    }

    #[test]
    fn whitespace_only_note_clears_the_sidecar() {
        let (_dir, library, id) = saved_reading();
        set_note(&library, &id, "keep me").unwrap();

        set_note(&library, &id, "  \n\t").unwrap();

        assert_eq!(get_note(&library, &id).unwrap(), None);
        assert!(!library.note_path(&id).exists());
    }

    #[test]
    fn unknown_reading_is_rejected_without_creating_a_folder() {
        let dir = TempDir::new().unwrap();
        let library = make_library(&dir);
        let id = new_id();

        assert!(set_note(&library, &id, "orphan").is_err());
        assert!(get_note(&library, &id).is_err());
        assert!(!library.reading_dir(&id).exists());
    }

    #[test]
    fn divergent_frontmatter_id_is_rejected() {
        let dir = TempDir::new().unwrap();
        let library = make_library(&dir);
        let requested = new_id();
        let actual = new_id();
        fs::create_dir_all(library.reading_dir(&requested)).unwrap();
        let content = crate::render_reading(&crate::Reading {
            metadata: metadata(&actual),
            body: "body".into(),
        })
        .unwrap();
        fs::write(library.article_path(&requested), content).unwrap();

        assert!(set_note(&library, &requested, "must not write").is_err());
        assert!(!library.note_path(&requested).exists());
    }

    #[test]
    fn malformed_ids_are_rejected_before_path_resolution() {
        let (_dir, library, id) = saved_reading();

        for malformed in ["", "..", "../../..", "a/b", "."] {
            assert!(set_note(&library, malformed, "must not write").is_err());
            assert!(get_note(&library, malformed).is_err());
        }

        assert!(library.article_path(&id).is_file());
        assert!(!library.note_path(&id).exists());
    }

    #[test]
    fn article_rewrites_leave_the_note_untouched() {
        let (_dir, library, id) = saved_reading();
        let note = "A **personal** annotation";
        set_note(&library, &id, note).unwrap();

        write_reading(&library, metadata(&id), "Updated captured body".into()).unwrap();

        assert_eq!(get_note(&library, &id).unwrap().as_deref(), Some(note));
    }

    #[test]
    fn concurrent_writers_leave_one_complete_note() {
        use std::sync::{Arc, Barrier};

        let (dir, library, id) = saved_reading();
        let root = dir.path().to_path_buf();
        let payloads = (0..8)
            .map(|index| format!("writer-{index}\n{}", index.to_string().repeat(100_000)))
            .collect::<Vec<_>>();
        let barrier = Arc::new(Barrier::new(payloads.len()));
        let handles = payloads
            .iter()
            .cloned()
            .map(|payload| {
                let root = root.clone();
                let id = id.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let library = LibraryRoot::new(root).unwrap();
                    barrier.wait();
                    set_note(&library, &id, &payload).unwrap();
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }

        let note = get_note(&library, &id).unwrap().unwrap();
        assert!(payloads.contains(&note));
        assert!(!fs::read_dir(library.reading_dir(&id))
            .unwrap()
            .any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            }));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_note_is_never_read_or_written_through() {
        use std::os::unix::fs::symlink;

        let (dir, library, id) = saved_reading();
        let outside = dir.path().join("outside.md");
        fs::write(&outside, "outside secret").unwrap();
        symlink(&outside, library.note_path(&id)).unwrap();

        assert!(get_note(&library, &id).is_err());
        set_note(&library, &id, "safe replacement").unwrap();

        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside secret");
        assert_eq!(
            get_note(&library, &id).unwrap().as_deref(),
            Some("safe replacement")
        );
    }
}
