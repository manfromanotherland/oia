// SPDX-License-Identifier: MIT

//! Selection saves distinguish an explicit quotation from a highlighted passage.
//! The browser supplies evidence and the shared core owns interpretation, article
//! identity, quote Markdown, and file-backed featured-highlight state.

use crate::{LibraryRoot, ReadingKind, SaveDisposition, SaveError, SaveInput, SaveOutcome};
use serde::{Deserialize, Serialize};

pub const MAX_SELECTION_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionContext {
    pub version: u32,
    pub selected_text: String,
    #[serde(default)]
    pub enclosing_element: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SelectionKind {
    Quote,
    Highlight,
}

impl SelectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quote => "quote",
            Self::Highlight => "highlight",
        }
    }
}

pub struct SelectionSaveInput {
    pub capture: SaveInput,
    pub selected_text: String,
    pub context: Option<SelectionContext>,
}

/// Uncertain selections are highlights. Quotation marks must surround the
/// entire selection; a quote-like word or punctuation inside prose is not
/// enough. DOM evidence is used only when its version and selected text match.
pub fn selection_kind(selected_text: &str, context: Option<&SelectionContext>) -> SelectionKind {
    let text = selected_text.trim();
    if text.is_empty() {
        return SelectionKind::Highlight;
    }
    let tag_evidence = context
        .filter(|context| {
            context.version == 1 && normalized_text(&context.selected_text) == normalized_text(text)
        })
        .and_then(|context| context.enclosing_element.as_deref())
        .is_some_and(|tag| matches!(tag.trim().to_ascii_lowercase().as_str(), "blockquote" | "q"));
    let marks_evidence = [
        ('"', '"'),
        ('\'', '\''),
        ('“', '”'),
        ('‘', '’'),
        ('«', '»'),
        ('‹', '›'),
    ]
    .iter()
    .any(|(open, close)| enclosed_quote(text, *open, *close));
    if tag_evidence || marks_evidence {
        SelectionKind::Quote
    } else {
        SelectionKind::Highlight
    }
}

fn enclosed_quote(text: &str, open: char, close: char) -> bool {
    let Some(inside) = text
        .strip_prefix(open)
        .and_then(|text| text.strip_suffix(close))
        .filter(|text| !text.trim().is_empty())
    else {
        return false;
    };
    let characters: Vec<char> = inside.chars().collect();
    let mut depth = 0;
    for (index, character) in characters.iter().copied().enumerate() {
        // Apostrophes within words do not terminate a quoted passage.
        let apostrophe = (close == '\'' || close == '’')
            && character == close
            && index > 0
            && index + 1 < characters.len()
            && characters[index - 1].is_alphanumeric()
            && characters[index + 1].is_alphanumeric();
        if apostrophe {
            continue;
        }
        if open == close {
            if character == close {
                return false;
            }
        } else if character == open {
            depth += 1;
        } else if character == close {
            if depth == 0 {
                return false;
            }
            depth -= 1;
        }
    }
    depth == 0
}

fn normalized_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Save a quote at the established quote identity or attach a passage to the
/// URL-derived article. Existing full captures and their user state are kept;
/// lightweight links upgrade only when extracted article content is available.
pub fn save_selection(
    library: &LibraryRoot,
    input: SelectionSaveInput,
) -> Result<SaveOutcome, SaveError> {
    let SelectionSaveInput {
        mut capture,
        selected_text,
        context,
    } = input;
    let selected = selected_text.trim();
    if selected.is_empty() || selected_text.len() > MAX_SELECTION_TEXT_BYTES {
        return Err(SaveError::InvalidRequest(
            "selection must contain 1–65536 bytes of text".into(),
        ));
    }
    if context.as_ref().is_some_and(|context| {
        context.selected_text.len() > MAX_SELECTION_TEXT_BYTES
            || context
                .enclosing_element
                .as_ref()
                .is_some_and(|tag| tag.len() > 32)
    }) {
        return Err(SaveError::InvalidRequest(
            "selection context exceeded its bounds".into(),
        ));
    }
    capture.media_url = None;
    if selection_kind(selected, context.as_ref()) == SelectionKind::Quote {
        capture.kind = ReadingKind::Quote;
        capture.lightweight = false;
        capture.quote_identity_markdown = Some(selected.to_string());
        capture.markdown = selected
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        capture.excerpt = Some(selected.chars().take(600).collect());
        capture.word_count = Some(
            selected
                .split_whitespace()
                .count()
                .try_into()
                .unwrap_or(u32::MAX),
        );
        capture.product = None;
        return crate::save_capture(library, capture);
    }
    capture.kind = ReadingKind::Article;
    capture.quote_identity_markdown = None;
    if capture.lightweight || capture.markdown.trim().is_empty() {
        capture.lightweight = true;
        let normalized = crate::normalize_url(&capture.url)
            .map_err(|error| SaveError::InvalidRequest(error.to_string()))?;
        capture.markdown = format!("[Open link](<{normalized}>)");
        capture.word_count = None;
    }
    let mut outcome = crate::save_capture(library, capture)?;
    crate::highlights::add_featured_highlight(library, &outcome.id, selected)?;
    // A duplicate article is still a successful selection save: the passage
    // exists durably and re-saving it features its stable existing ID.
    if outcome.disposition == SaveDisposition::Duplicate {
        outcome.disposition = SaveDisposition::Upgraded;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn capture(url: &str, body: &str) -> SaveInput {
        SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Article,
            lightweight: false,
            url: url.into(),
            media_url: None,
            canonical_url: url.into(),
            title: "Article title".into(),
            author: None,
            site: Some("example.com".into()),
            saved_at: "2026-10-10T12:00:00.000Z".into(),
            markdown: body.into(),
            images: vec![],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            product: None,
            excerpt: None,
            word_count: None,
            lang: None,
        }
    }
    fn save(library: &LibraryRoot, body: &str, text: &str) -> SaveOutcome {
        save_selection(
            library,
            SelectionSaveInput {
                capture: capture("https://example.com/article", body),
                selected_text: text.into(),
                context: None,
            },
        )
        .unwrap()
    }
    #[test]
    fn quote_requires_enclosing_marks_or_matching_dom_evidence() {
        for text in [
            "\"A quotation.\"",
            "“A quotation.”",
            "‘A quotation.’",
            "«A quotation.»",
        ] {
            assert_eq!(selection_kind(text, None), SelectionKind::Quote);
        }
        for text in [
            "A sentence with “a quoted phrase” inside.",
            "A sentence. Another sentence!",
            "it's prose",
            "“unmatched quote",
            "\"\"",
            "\"First quote\" then prose \"second quote\"",
            "“First quote” then prose “second quote”",
        ] {
            assert_eq!(selection_kind(text, None), SelectionKind::Highlight);
        }
        let context = SelectionContext {
            version: 1,
            selected_text: "A \n passage.".into(),
            enclosing_element: Some("BLOCKQUOTE".into()),
        };
        assert_eq!(
            selection_kind("A passage.", Some(&context)),
            SelectionKind::Quote
        );
        assert_eq!(
            selection_kind("A different passage.", Some(&context)),
            SelectionKind::Highlight
        );
        assert_eq!(
            selection_kind(
                "A passage.",
                Some(&SelectionContext {
                    version: 2,
                    ..context.clone()
                })
            ),
            SelectionKind::Highlight
        );
        assert_eq!(
            selection_kind(
                "A passage.",
                Some(&SelectionContext {
                    enclosing_element: Some("p".into()),
                    ..context
                })
            ),
            SelectionKind::Highlight
        );
    }
    #[test]
    fn ordinary_selection_preserves_full_article_and_existing_state() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let initial = crate::save_capture(
            &library,
            capture("https://example.com/article", "Original complete body."),
        )
        .unwrap();
        let path = library.article_path(&initial.id);
        let mut reading = crate::parse_reading(&fs::read_to_string(&path).unwrap()).unwrap();
        reading.metadata.tags = vec!["kept".into()];
        reading.metadata.favorite = true;
        crate::write_reading(&library, reading.metadata.clone(), reading.body.clone()).unwrap();
        let before_content = fs::read_to_string(&path).unwrap();
        let before_metadata = crate::parse_reading(&before_content).unwrap().metadata;
        let expected_body = before_content.split_once("\n---\n").unwrap().1.to_string();
        let saved = save(
            &library,
            "Replacement should be ignored.",
            "Highlighted passage.",
        );
        assert_eq!(saved.id, initial.id);
        assert_eq!(saved.disposition, SaveDisposition::Upgraded);
        let content = fs::read_to_string(&path).unwrap();
        let updated = crate::parse_reading(&content).unwrap();
        assert_eq!(content.split_once("\n---\n").unwrap().1, expected_body);
        assert_eq!(updated.metadata.tags, vec!["kept"]);
        assert!(updated.metadata.favorite);
        assert_eq!(updated.metadata.saved_at, reading.metadata.saved_at);
        assert_eq!(updated.metadata.source_hash, before_metadata.source_hash);
        let highlights = crate::list_highlights(&library, &saved.id).unwrap();
        assert_eq!(
            updated.metadata.featured_highlight_id.as_deref(),
            Some(highlights[0].id.as_str())
        );
    }
    #[test]
    fn repeated_and_multiple_passages_feature_stable_ids() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = save(&library, "Full article.", "First passage.");
        save(&library, "Other capture.", "Second passage.");
        save(&library, "Other capture.", "First passage.");
        let highlights = crate::list_highlights(&library, &saved.id).unwrap();
        assert_eq!(highlights.len(), 2);
        let metadata = crate::read_metadata(&library.article_path(&saved.id)).unwrap();
        assert_eq!(
            metadata.featured_highlight_id.as_deref(),
            Some(highlights[0].id.as_str())
        );
        crate::delete_highlight(&library, &saved.id, &highlights[0].id).unwrap();
        let metadata = crate::read_metadata(&library.article_path(&saved.id)).unwrap();
        assert_eq!(
            metadata.featured_highlight_id.as_deref(),
            Some(highlights[1].id.as_str())
        );
        assert!(!crate::toggle_highlight(&library, &saved.id, "Second passage.").unwrap());
        assert!(crate::read_metadata(&library.article_path(&saved.id))
            .unwrap()
            .featured_highlight_id
            .is_none());
    }
    #[test]
    fn unavailable_extraction_saves_lightweight_and_later_capture_upgrades() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = save(&library, "", "First passage.");
        let before = crate::read_metadata(&library.article_path(&saved.id)).unwrap();
        assert!(before.lightweight);
        assert_eq!(before.kind, ReadingKind::Article);
        save(
            &library,
            "Full article from later capture.",
            "Second passage.",
        );
        let reading =
            crate::parse_reading(&fs::read_to_string(library.article_path(&saved.id)).unwrap())
                .unwrap();
        assert!(!reading.metadata.lightweight);
        assert_eq!(reading.body.trim(), "Full article from later capture.");
        assert_eq!(
            crate::list_highlights(&library, &saved.id).unwrap().len(),
            2
        );
        assert!(reading.metadata.featured_highlight_id.is_some());
    }
    #[test]
    fn quote_markdown_and_identity_are_owned_by_core() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let input = SelectionSaveInput {
            capture: capture(
                "https://example.com/article",
                "Full article unrelated to quote.",
            ),
            selected_text: "A quotation\non two lines.".into(),
            context: Some(SelectionContext {
                version: 1,
                selected_text: "A quotation on two lines.".into(),
                enclosing_element: Some("q".into()),
            }),
        };
        let saved = save_selection(&library, input).unwrap();
        let reading =
            crate::parse_reading(&fs::read_to_string(library.article_path(&saved.id)).unwrap())
                .unwrap();
        assert_eq!(reading.metadata.kind, ReadingKind::Quote);
        assert_eq!(reading.body.trim(), "> A quotation\n> on two lines.");
        assert_eq!(
            saved.id,
            crate::quote_id("https://example.com/article", "A quotation\non two lines.").unwrap()
        );
        assert!(crate::list_highlights(&library, &saved.id)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn sidecar_rebuild_and_external_featured_edits_refresh_projection() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = save(&library, "Full article.", "First passage.");
        let index_dir = tempfile::tempdir().unwrap();
        let database =
            crate::ffi::Database::open(index_dir.path().join("index.db").display().to_string())
                .unwrap();
        let library_path = dir.path().display().to_string();
        database.sync(library_path.clone()).unwrap();
        assert_eq!(
            database
                .get_reading_row(saved.id.clone())
                .unwrap()
                .unwrap()
                .featured_highlight_text
                .as_deref(),
            Some("First passage.")
        );
        let sidecar = library.highlights_path(&saved.id);
        let original = fs::read_to_string(&sidecar).unwrap();
        fs::write(
            &sidecar,
            original.replace("First passage.", "Externally changed passage."),
        )
        .unwrap();
        assert_eq!(
            database
                .sync_paths(library_path.clone(), vec![sidecar.display().to_string()])
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .get_reading_row(saved.id.clone())
                .unwrap()
                .unwrap()
                .featured_highlight_text
                .as_deref(),
            Some("Externally changed passage.")
        );
        let rebuilt = crate::open_index(&index_dir.path().join("rebuilt.db")).unwrap();
        crate::rebuild(&rebuilt, &library).unwrap();
        assert_eq!(
            crate::get_reading(&rebuilt, &saved.id)
                .unwrap()
                .unwrap()
                .0
                .featured_highlight_text
                .as_deref(),
            Some("Externally changed passage.")
        );
        fs::remove_file(&sidecar).unwrap();
        assert_eq!(
            database
                .sync_paths(library_path, vec![sidecar.display().to_string()])
                .unwrap(),
            1
        );
        assert!(database
            .get_reading_row(saved.id)
            .unwrap()
            .unwrap()
            .featured_highlight_text
            .is_none());
    }
    #[test]
    fn ffi_delete_and_toggle_reconcile_featured_preview_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = save(&library, "Full article.", "First passage.");
        save(&library, "Ignored full capture.", "Second passage.");
        let highlights = crate::list_highlights(&library, &saved.id).unwrap();
        let index_dir = tempfile::tempdir().unwrap();
        let database =
            crate::ffi::Database::open(index_dir.path().join("index.db").display().to_string())
                .unwrap();
        let library_path = dir.path().display().to_string();
        database.sync(library_path.clone()).unwrap();
        assert_eq!(
            database
                .get_reading_row(saved.id.clone())
                .unwrap()
                .unwrap()
                .featured_highlight_text
                .as_deref(),
            Some("Second passage.")
        );
        database
            .delete_highlight(
                library_path.clone(),
                saved.id.clone(),
                highlights[1].id.clone(),
            )
            .unwrap();
        assert_eq!(
            database
                .get_reading_row(saved.id.clone())
                .unwrap()
                .unwrap()
                .featured_highlight_text
                .as_deref(),
            Some("First passage.")
        );
        assert!(!database
            .toggle_highlight(
                library_path.clone(),
                saved.id.clone(),
                "First passage.".into()
            )
            .unwrap());
        assert!(database
            .get_reading_row(saved.id.clone())
            .unwrap()
            .unwrap()
            .featured_highlight_text
            .is_none());
        database
            .add_highlight(library_path, saved.id.clone(), "Third passage.".into())
            .unwrap();
        assert_eq!(
            database
                .get_reading_row(saved.id)
                .unwrap()
                .unwrap()
                .featured_highlight_text
                .as_deref(),
            Some("Third passage.")
        );
    }
    #[test]
    fn browser_full_capture_upgrade_preserves_featured_id_and_passage() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = save(&library, "", "First passage.");
        let before = crate::read_metadata(&library.article_path(&saved.id)).unwrap();
        crate::save_capture(
            &library,
            capture("https://example.com/article", "Full browser article."),
        )
        .unwrap();
        let after = crate::read_metadata(&library.article_path(&saved.id)).unwrap();
        assert_eq!(after.featured_highlight_id, before.featured_highlight_id);
        assert!(!after.lightweight);
        let conn =
            crate::open_index(&tempfile::tempdir().unwrap().path().join("index.db")).unwrap();
        crate::rebuild(&conn, &library).unwrap();
        assert_eq!(
            crate::get_reading(&conn, &saved.id)
                .unwrap()
                .unwrap()
                .0
                .featured_highlight_text
                .as_deref(),
            Some("First passage.")
        );
    }
    #[test]
    fn feature_update_preserves_unknown_yaml_and_raw_markdown_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let saved = crate::save_capture(
            &library,
            capture("https://example.com/article", "Full article."),
        )
        .unwrap();
        let path = library.article_path(&saved.id);
        let original = fs::read_to_string(&path).unwrap();
        let (header, body) = original.split_once("\n---\n").unwrap();
        let original=format!("{header}\nfuture_metadata: 'Keep this exact value.'\n# top-level comment\n---\n{body}\n\nTrailing spaces  \n\n");
        fs::write(&path, &original).unwrap();
        save(&library, "Ignored replacement.", "Featured passage.");
        let updated = fs::read_to_string(path).unwrap();
        assert!(updated.contains("future_metadata: 'Keep this exact value.'\n# top-level comment"));
        assert_eq!(
            updated.split_once("\n---\n").unwrap().1,
            original.split_once("\n---\n").unwrap().1
        );
    }

    #[test]
    fn quote_excerpt_is_bounded_by_characters_without_losing_selected_content() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let text = format!("“{}”", "é💡".repeat(500));
        let saved = save_selection(
            &library,
            SelectionSaveInput {
                capture: capture("https://example.com/article", "Full article."),
                selected_text: text.clone(),
                context: None,
            },
        )
        .unwrap();
        let reading =
            crate::parse_reading(&fs::read_to_string(library.article_path(&saved.id)).unwrap())
                .unwrap();
        assert_eq!(reading.metadata.excerpt.unwrap().chars().count(), 600);
        assert!(reading.body.contains(&text));
        assert_eq!(
            saved.id,
            crate::quote_id("https://example.com/article", &text).unwrap()
        );
    }
}
