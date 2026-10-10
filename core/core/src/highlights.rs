// SPDX-License-Identifier: MIT

//! Per-reading text highlights, stored as a Markdown document.
//!
//! Each reading's highlights live in `highlights.md` inside the reading's own
//! folder, beside its `article.md`. Featured passage text is projected into the
//! disposable SQLite index, and the
//! scanner keys on the fixed `article.md` filename, so it never mistakes the
//! highlights file for a reading. A highlight is the verbatim text the user
//! selected in the reader; the file renders each as a Markdown block quote
//! followed by an HTML comment carrying a stable id, so highlights can be listed
//! and deleted individually while the file stays human-readable.

use std::{
    fs,
    io::{Read as _, Write as _},
};

use anyhow::{bail, Result};

use crate::{
    locking::{lock_reading, ReadingLock},
    new_id, LibraryRoot, Metadata, ReadingKind,
};

/// A single saved highlight: a stable id and the verbatim selected text.
#[derive(Debug, Clone, PartialEq)]
pub struct Highlight {
    pub id: String,
    pub text: String,
}

/// List a reading's highlights in creation order. A missing file (the common
/// case — most readings have none) yields an empty list.
pub fn list_highlights(library: &LibraryRoot, reading_id: &str) -> Result<Vec<Highlight>> {
    let lock = lock_reading(library, reading_id)?;
    list_highlights_under_lock(library, reading_id, &lock)
}

fn list_highlights_under_lock(
    library: &LibraryRoot,
    reading_id: &str,
    lock: &ReadingLock,
) -> Result<Vec<Highlight>> {
    lock.ensure_protects(library, reading_id)?;
    let path = library.highlights_path(reading_id);
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)?;
    Ok(parse(&content))
}

/// Append a highlight for `reading_id` and return it. Creates the `highlights/`
/// directory and the file on first use. Whitespace-only text is rejected.
/// Duplicates are never stored: re-adding an existing passage returns the
/// existing highlight unchanged.
pub fn add_highlight(library: &LibraryRoot, reading_id: &str, text: &str) -> Result<Highlight> {
    add_highlight_impl(library, reading_id, text, false)
}

/// Selection ingress requires an existing article before touching its sidecar.
pub(crate) fn add_featured_highlight(
    library: &LibraryRoot,
    reading_id: &str,
    text: &str,
) -> Result<Highlight> {
    add_highlight_impl(library, reading_id, text, true)
}

fn add_highlight_impl(
    library: &LibraryRoot,
    reading_id: &str,
    text: &str,
    require_article: bool,
) -> Result<Highlight> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("highlight text is empty");
    }
    let lock = lock_reading(library, reading_id)?;
    if require_article
        && crate::scanner::read_reading_for_id(library, reading_id)?
            .is_none_or(|reading| reading.metadata.kind != ReadingKind::Article)
    {
        bail!("article disappeared before the selection could be saved");
    }
    let mut highlights = list_highlights_under_lock(library, reading_id, &lock)?;
    let highlight = if let Some(existing) = highlights
        .iter()
        .find(|highlight| highlight.text == trimmed)
    {
        existing.clone()
    } else {
        let highlight = Highlight {
            id: new_id(),
            text: trimmed.into(),
        };
        highlights.push(highlight.clone());
        write_under_lock(library, reading_id, &highlights, &lock)?;
        highlight
    };
    update_featured_highlight_under_lock(library, reading_id, Some(&highlight.id), &lock)?;
    Ok(highlight)
}

/// Toggle a highlight by its text: if a highlight with this exact (trimmed)
/// text already exists it is removed and `false` is returned; otherwise it is
/// added and `true` is returned. This lets re-selecting an already-highlighted
/// passage clear it, and guarantees no duplicates.
pub fn toggle_highlight(library: &LibraryRoot, reading_id: &str, text: &str) -> Result<bool> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("highlight text is empty");
    }
    let lock = lock_reading(library, reading_id)?;
    let mut highlights = list_highlights_under_lock(library, reading_id, &lock)?;
    if let Some(pos) = highlights.iter().position(|h| h.text == trimmed) {
        highlights.remove(pos);
        if highlights.is_empty() {
            delete_all_highlights_under_lock(library, reading_id, &lock)?;
        } else {
            write_under_lock(library, reading_id, &highlights, &lock)?;
        }
        reconcile_featured_highlight_under_lock(library, reading_id, &highlights, &lock)?;
        Ok(false)
    } else {
        let highlight = Highlight {
            id: new_id(),
            text: trimmed.to_string(),
        };
        let featured_id = highlight.id.clone();
        highlights.push(highlight);
        write_under_lock(library, reading_id, &highlights, &lock)?;
        update_featured_highlight_under_lock(library, reading_id, Some(&featured_id), &lock)?;
        Ok(true)
    }
}

/// Remove the highlight with `highlight_id`. A no-op if it isn't present. When
/// the last highlight is removed, the now-empty file is deleted.
pub fn delete_highlight(library: &LibraryRoot, reading_id: &str, highlight_id: &str) -> Result<()> {
    let lock = lock_reading(library, reading_id)?;
    let mut highlights = list_highlights_under_lock(library, reading_id, &lock)?;
    let before = highlights.len();
    highlights.retain(|h| h.id != highlight_id);
    if highlights.len() == before {
        return Ok(());
    }
    if highlights.is_empty() {
        delete_all_highlights_under_lock(library, reading_id, &lock)?;
    } else {
        write_under_lock(library, reading_id, &highlights, &lock)?;
    }
    reconcile_featured_highlight_under_lock(library, reading_id, &highlights, &lock)
}

/// Remove a reading's entire highlights file. Used when the reading itself is
/// deleted. A no-op if the file does not exist.
pub fn delete_all_highlights(library: &LibraryRoot, reading_id: &str) -> Result<()> {
    let lock = lock_reading(library, reading_id)?;
    delete_all_highlights_under_lock(library, reading_id, &lock)?;
    update_featured_highlight_under_lock(library, reading_id, None, &lock)
}

fn delete_all_highlights_under_lock(
    library: &LibraryRoot,
    reading_id: &str,
    lock: &ReadingLock,
) -> Result<()> {
    lock.ensure_protects(library, reading_id)?;
    let path = library.highlights_path(reading_id);
    if path.is_file() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Resolve only the featured ID from the durable sidecar. Missing, externally
/// removed, or malformed passages produce no preview; the article stays valid.
pub(crate) fn featured_highlight_text(
    library: &LibraryRoot,
    metadata: &Metadata,
) -> Option<String> {
    if metadata.kind != ReadingKind::Article {
        return None;
    }
    let featured_id = metadata.featured_highlight_id.as_deref()?;
    let directory = crate::notes::open_reading_dir_components(library, &metadata.id).ok()?;
    let descriptor = rustix::fs::openat(
        &directory,
        "highlights.md",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .ok()?;
    let mut file = fs::File::from(descriptor);
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    parse(&text)
        .into_iter()
        .find(|highlight| highlight.id == featured_id && !highlight.text.trim().is_empty())
        .map(|highlight| highlight.text)
}

fn reconcile_featured_highlight_under_lock(
    library: &LibraryRoot,
    reading_id: &str,
    highlights: &[Highlight],
    lock: &ReadingLock,
) -> Result<()> {
    let Some(reading) = crate::scanner::read_reading_for_id(library, reading_id)? else {
        return Ok(());
    };
    if reading
        .metadata
        .featured_highlight_id
        .as_ref()
        .is_some_and(|id| !highlights.iter().any(|highlight| &highlight.id == id))
    {
        update_featured_highlight_under_lock(
            library,
            reading_id,
            highlights.last().map(|highlight| highlight.id.as_str()),
            lock,
        )?;
    }
    Ok(())
}

/// Patch just this scalar in frontmatter, preserving unknown YAML, Markdown
/// bytes, source_hash, and file permissions. The full snapshot is rechecked
/// immediately before rename so an external writer's changed file is retained.
fn update_featured_highlight_under_lock(
    library: &LibraryRoot,
    reading_id: &str,
    featured_id: Option<&str>,
    lock: &ReadingLock,
) -> Result<()> {
    lock.ensure_protects(library, reading_id)?;
    let Some(reading) = crate::scanner::read_reading_for_id(library, reading_id)? else {
        return Ok(());
    };
    if reading.metadata.kind != ReadingKind::Article
        || reading.metadata.featured_highlight_id.as_deref() == featured_id
    {
        return Ok(());
    }
    let path = library.article_path(reading_id);
    let original = fs::read_to_string(&path)?;
    let parsed = crate::parse_reading(&original)?;
    if parsed.metadata.id != reading_id || parsed.metadata.kind != ReadingKind::Article {
        bail!("reading changed while featuring a highlight");
    }
    let rest = original
        .strip_prefix("---\n")
        .ok_or_else(|| anyhow::anyhow!("missing frontmatter"))?;
    let close = rest
        .find("\n---\n")
        .ok_or_else(|| anyhow::anyhow!("missing frontmatter close"))?;
    let mut retained = String::new();
    let mut skipping = false;
    for line in rest[..close].split_inclusive('\n') {
        let top_level = !line.starts_with([' ', '\t']) && !line.trim().is_empty();
        if top_level && !line.starts_with('#') {
            skipping = line.split_once(':').is_some_and(|(key, _)| {
                key.trim().trim_matches(['\'', '"']) == "featured_highlight_id"
            });
        }
        if !skipping || (top_level && line.starts_with('#')) {
            retained.push_str(line);
        }
    }
    if !retained.ends_with('\n') {
        retained.push('\n');
    }
    if let Some(id) = featured_id {
        retained.push_str("featured_highlight_id: ");
        retained.push_str(&serde_yaml::to_string(id)?);
    }
    let updated = format!("---\n{retained}{}", &rest[close + 1..]);
    let mut staged = tempfile::NamedTempFile::new_in(library.reading_dir(reading_id))?;
    staged
        .as_file()
        .set_permissions(fs::metadata(&path)?.permissions())?;
    staged.write_all(updated.as_bytes())?;
    staged.as_file().sync_all()?;
    if fs::read_to_string(&path)? != original {
        bail!(
            "reading changed while featuring a highlight; selection was retained in highlights.md"
        );
    }
    staged.persist(&path).map_err(|error| error.error)?;
    fs::File::open(library.reading_dir(reading_id))?.sync_all()?;
    Ok(())
}

// ── Serialization ────────────────────────────────────────────────────────────

/// Render highlights as a Markdown document. Each highlight is a block quote
/// (one `> ` line per text line) terminated by `<!-- hl {id} -->`.
fn write_under_lock(
    library: &LibraryRoot,
    reading_id: &str,
    highlights: &[Highlight],
    lock: &ReadingLock,
) -> Result<()> {
    lock.ensure_protects(library, reading_id)?;
    // The highlights file lives inside the reading's folder; create it in case a
    // highlight is somehow saved before the article file exists.
    std::fs::create_dir_all(library.reading_dir(reading_id))?;
    let mut out = String::new();
    for h in highlights {
        if h.text.is_empty() {
            out.push_str(">\n");
        } else {
            for line in h.text.split('\n') {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str("<!-- hl ");
        out.push_str(&h.id);
        out.push_str(" -->\n\n");
    }
    let path = library.highlights_path(reading_id);
    let tmp_path = path.with_file_name("highlights.md.tmp");
    {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(out.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

/// Parse the block-quote-plus-marker format produced by `write`. Lines starting
/// with `> ` (or a bare `>`) accumulate into the current highlight; an
/// `<!-- hl {id} -->` line closes it. Anything else (blank lines) is ignored.
/// Rendered text lines are always `> `-prefixed, so a quote whose own text
/// begins with `<!-- hl ` cannot be confused for a terminator.
fn parse(content: &str) -> Vec<Highlight> {
    let mut result = Vec::new();
    let mut buf: Vec<String> = Vec::new();
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("<!-- hl ") {
            let id = rest.trim_end_matches("-->").trim().to_string();
            if !id.is_empty() {
                result.push(Highlight {
                    id,
                    text: buf.join("\n"),
                });
            }
            buf.clear();
        } else if let Some(rest) = line.strip_prefix("> ") {
            buf.push(rest.to_string());
        } else if line == ">" {
            buf.push(String::new());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn make_library(dir: &TempDir) -> LibraryRoot {
        fs::create_dir_all(dir.path().join("articles")).unwrap();
        LibraryRoot::new(dir.path()).unwrap()
    }

    #[test]
    fn missing_file_lists_empty() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        assert!(list_highlights(&lib, "no-such-reading").unwrap().is_empty());
    }

    #[test]
    fn add_then_list_roundtrips() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        let a = add_highlight(&lib, "r1", "first passage").unwrap();
        let b = add_highlight(&lib, "r1", "second passage").unwrap();

        let listed = list_highlights(&lib, "r1").unwrap();
        assert_eq!(listed, vec![a, b]);
    }

    #[test]
    fn preserves_multi_line_text() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let text = "line one\nline two\nline three";
        add_highlight(&lib, "r1", text).unwrap();

        let listed = list_highlights(&lib, "r1").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].text, text);
    }

    #[test]
    fn empty_text_is_rejected() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        assert!(add_highlight(&lib, "r1", "   \n  ").is_err());
    }

    #[test]
    fn delete_removes_one_highlight() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let a = add_highlight(&lib, "r1", "keep me").unwrap();
        let b = add_highlight(&lib, "r1", "remove me").unwrap();

        delete_highlight(&lib, "r1", &b.id).unwrap();

        let listed = list_highlights(&lib, "r1").unwrap();
        assert_eq!(listed, vec![a]);
    }

    #[test]
    fn deleting_last_highlight_removes_file() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let a = add_highlight(&lib, "r1", "only one").unwrap();

        delete_highlight(&lib, "r1", &a.id).unwrap();

        assert!(!lib.highlights_path("r1").exists());
        assert!(list_highlights(&lib, "r1").unwrap().is_empty());
    }

    #[test]
    fn delete_unknown_id_is_noop() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let a = add_highlight(&lib, "r1", "stays").unwrap();

        delete_highlight(&lib, "r1", "not-a-real-id").unwrap();

        assert_eq!(list_highlights(&lib, "r1").unwrap(), vec![a]);
    }

    #[test]
    fn delete_all_clears_file() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        add_highlight(&lib, "r1", "one").unwrap();
        add_highlight(&lib, "r1", "two").unwrap();

        delete_all_highlights(&lib, "r1").unwrap();

        assert!(!lib.highlights_path("r1").exists());
    }

    #[test]
    fn adding_same_text_does_not_duplicate() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let first = add_highlight(&lib, "r1", "same passage").unwrap();
        // Re-adding (with surrounding whitespace, which is trimmed) returns the
        // existing highlight and does not create a second entry.
        let again = add_highlight(&lib, "r1", "  same passage  ").unwrap();

        assert_eq!(first, again);
        assert_eq!(list_highlights(&lib, "r1").unwrap(), vec![first]);
    }

    #[test]
    fn toggle_adds_then_removes_same_text() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);

        assert!(toggle_highlight(&lib, "r1", "a passage").unwrap());
        assert_eq!(list_highlights(&lib, "r1").unwrap().len(), 1);

        // Toggling the same text again removes it.
        assert!(!toggle_highlight(&lib, "r1", "a passage").unwrap());
        assert!(list_highlights(&lib, "r1").unwrap().is_empty());
        assert!(!lib.highlights_path("r1").exists());
    }

    #[test]
    fn toggle_off_keeps_other_highlights() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        let keep = add_highlight(&lib, "r1", "keep").unwrap();
        add_highlight(&lib, "r1", "drop").unwrap();

        assert!(!toggle_highlight(&lib, "r1", "drop").unwrap());

        assert_eq!(list_highlights(&lib, "r1").unwrap(), vec![keep]);
    }

    #[test]
    fn text_containing_marker_like_line_roundtrips() {
        let dir = TempDir::new().unwrap();
        let lib = make_library(&dir);
        // A quote whose text itself looks like the terminator must survive,
        // because rendered text lines are always `> `-prefixed.
        let text = "<!-- hl fake -->";
        add_highlight(&lib, "r1", text).unwrap();

        let listed = list_highlights(&lib, "r1").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].text, text);
    }
}
