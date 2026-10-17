// SPDX-License-Identifier: MIT

use anyhow::Result;
use oia_core::{
    save_selection, selection_kind, LibraryRoot, ReadingKind, SaveError, SaveInput,
    SelectionSaveInput,
};

use crate::{
    protocol::{SaveResponse, SaveSelectionRequest, PROTOCOL_VERSION},
    save::{decode_images, detected_preview_url, find_library_path, response_for_outcome},
};

const MAX_SELECTED_TEXT_BYTES: usize = oia_core::MAX_SELECTION_TEXT_BYTES;
const MAX_CONTEXT_ELEMENT_BYTES: usize = 32;

pub fn handle(request: SaveSelectionRequest) -> Result<SaveResponse> {
    if let Some(response) = validate_request(&request) {
        return Ok(response);
    }
    let library_path = match find_library_path() {
        Ok(path) => path,
        Err(_) => {
            return Ok(SaveResponse::error(
                "library_not_configured",
                "No library folder has been set. Open the Óia app to configure one.",
            ))
        }
    };
    let library = LibraryRoot::new(library_path)?;
    handle_in_library(request, &library)
}

fn validate_request(request: &SaveSelectionRequest) -> Option<SaveResponse> {
    let capture = &request.capture;
    let invalid = |message: &str| Some(SaveResponse::error("invalid_request", message));
    if capture.protocol_version != PROTOCOL_VERSION {
        return invalid(&format!(
            "unsupported protocol_version: {}",
            capture.protocol_version
        ));
    }
    if capture.action != "save_selection" {
        return invalid(&format!("unknown action: {}", capture.action));
    }
    if capture.metadata.kind != ReadingKind::Article {
        return invalid("selection saves require article metadata");
    }
    if request.selected_text.len() > MAX_SELECTED_TEXT_BYTES {
        return invalid("selected text exceeded 64 KiB");
    }
    if request.selection_context.as_ref().is_some_and(|context| {
        context.selected_text.len() > MAX_SELECTED_TEXT_BYTES
            || context
                .enclosing_element
                .as_ref()
                .is_some_and(|element| element.len() > MAX_CONTEXT_ELEMENT_BYTES)
    }) {
        return invalid("selection context exceeded its size limit");
    }
    if capture
        .metadata
        .product_html
        .as_ref()
        .is_some_and(|html| html.len() > oia_core::product::MAX_BROWSER_PRODUCT_HTML_BYTES)
    {
        return invalid("product metadata evidence exceeded 512 KiB");
    }
    None
}

fn handle_in_library(request: SaveSelectionRequest, library: &LibraryRoot) -> Result<SaveResponse> {
    if let Some(response) = validate_request(&request) {
        return Ok(response);
    }
    let kind = selection_kind(&request.selected_text, request.selection_context.as_ref());
    let capture = request.capture;
    let detected = oia_core::parse_page_metadata(
        capture.metadata.product_html.as_deref().unwrap_or(""),
        &capture.metadata.url,
    );
    let images = decode_images(&capture.images);
    let preview_url =
        detected_preview_url(detected.image_url.as_deref(), &images, capture.preview_url);
    let input = SelectionSaveInput {
        selected_text: request.selected_text,
        context: request.selection_context,
        capture: SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Article,
            lightweight: capture.markdown.trim().is_empty(),
            url: capture.metadata.url,
            media_url: capture.metadata.media_url,
            canonical_url: capture.metadata.canonical_url,
            title: detected.title.unwrap_or(capture.metadata.title),
            author: detected
                .link
                .as_ref()
                .and_then(|value| value.author.clone())
                .or(capture.metadata.author),
            site: capture.metadata.site,
            saved_at: capture.metadata.saved_at,
            markdown: capture.markdown,
            images,
            preview_url,
            favicon_url: capture.favicon_url,
            theme_color: capture.metadata.theme_color,
            product: detected.product,
            link_category: detected.link.map(|value| value.category),
            excerpt: capture.metadata.excerpt,
            word_count: capture.metadata.word_count,
            lang: capture.metadata.lang,
        },
    };
    let outcome = match save_selection(library, input) {
        Ok(outcome) => outcome,
        Err(SaveError::InvalidRequest(message)) => {
            return Ok(SaveResponse::error("invalid_request", &message))
        }
        Err(SaveError::Storage(error)) => return Err(error),
    };
    let mut response = response_for_outcome(outcome);
    if response.ok {
        response.selection_kind = Some(kind);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use oia_core::{import_link, list_highlights, parse_reading, render_reading, url_id};
    use serde_json::{json, Value};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn selection_capture_attaches_cultural_metadata_and_cover_to_its_article() {
        let temporary = TempDir::new().unwrap();
        let library = LibraryRoot::new(temporary.path()).unwrap();
        let mut value = message(
            "https://example.com/book",
            "Selected ordinary passage.",
            None,
        );
        value["metadata"]["product_html"] = json!(
            r#"<script type="application/ld+json">{"@type":"Book","name":"Selected book","image":"https://example.com/cover.png"}</script>"#
        );
        value["images"] = json!([{"url":"https://example.com/cover.png","content_type":"image/png","data_base64":"Y292ZXI="}]);
        let response = handle_in_library(request(value), &library).unwrap();
        assert!(response.ok);
        assert_eq!(
            response.selection_kind,
            Some(oia_core::SelectionKind::Highlight)
        );
        let id = response.id.unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&id)).unwrap()).unwrap();
        assert_eq!(reading.metadata.link_category.as_deref(), Some("book"));
        assert_eq!(reading.metadata.title, "Selected book");
        assert_eq!(
            fs::read(
                library
                    .reading_dir(&id)
                    .join(reading.metadata.preview_asset.unwrap())
            )
            .unwrap(),
            b"cover"
        );
        assert_eq!(
            list_highlights(&library, &id).unwrap()[0].text,
            "Selected ordinary passage."
        );
    }

    #[test]
    fn quoted_selection_does_not_inherit_cultural_category() {
        let temporary = TempDir::new().unwrap();
        let library = LibraryRoot::new(temporary.path()).unwrap();
        let mut value = message("https://example.com/book", "“A selected quotation.”", None);
        value["metadata"]["product_html"] = json!(
            r#"<script type="application/ld+json">{"@type":"Book","name":"Selected book"}</script>"#
        );
        let response = handle_in_library(request(value), &library).unwrap();
        assert!(response.ok);
        let reading = parse_reading(
            &fs::read_to_string(library.article_path(&response.id.unwrap())).unwrap(),
        )
        .unwrap();
        assert_eq!(reading.metadata.kind, ReadingKind::Quote);
        assert!(reading.metadata.link_category.is_none());
    }

    fn message(url: &str, selected_text: &str, context: Option<Value>) -> Value {
        json!({
            "protocol_version": 4,
            "action": "save_selection",
            "metadata": {
                "url": url, "canonical_url": url, "title": "Full article",
                "site": "Example", "saved_at": "2026-10-10T12:00:00Z",
            },
            "markdown": "# Full article\n\nFirst paragraph.\n\nSelected ordinary passage.\n\nLast paragraph.\n",
            "images": [],
            "selected_text": selected_text,
            "selection_context": context,
        })
    }

    fn request(value: Value) -> SaveSelectionRequest {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn ordinary_selection_saves_whole_article_and_a_highlight() {
        let temporary = TempDir::new().unwrap();
        let library = LibraryRoot::new(temporary.path()).unwrap();
        let url = "https://example.com/article";
        let response = handle_in_library(
            request(message(url, "Selected ordinary passage.", None)),
            &library,
        )
        .unwrap();
        assert!(response.ok);
        assert_eq!(
            serde_json::to_value(&response).unwrap()["selection_kind"],
            "highlight"
        );
        assert_eq!(response.id.as_deref(), Some(url_id(url).unwrap().as_str()));
        let reading = parse_reading(
            &fs::read_to_string(library.article_path(response.id.as_deref().unwrap())).unwrap(),
        )
        .unwrap();
        assert_eq!(reading.metadata.kind, ReadingKind::Article);
        assert!(!reading.metadata.lightweight);
        assert!(reading.body.contains("First paragraph."));
        assert!(reading.body.contains("Last paragraph."));
        let highlights = list_highlights(&library, response.id.as_deref().unwrap()).unwrap();
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].text, "Selected ordinary passage.");
    }

    #[test]
    fn quoted_text_or_matching_quote_markup_saves_a_quote() {
        for (selected, context) in [
            ("“A distinct quotation.”", None),
            (
                "A distinct quotation.",
                Some(
                    json!({"version": 1, "selected_text": "A distinct quotation.", "enclosing_element": "blockquote"}),
                ),
            ),
        ] {
            let temporary = TempDir::new().unwrap();
            let library = LibraryRoot::new(temporary.path()).unwrap();
            let response = handle_in_library(
                request(message("https://example.com/article", selected, context)),
                &library,
            )
            .unwrap();
            assert!(response.ok);
            assert_eq!(
                serde_json::to_value(&response).unwrap()["selection_kind"],
                "quote"
            );
            let reading = parse_reading(
                &fs::read_to_string(library.article_path(response.id.as_deref().unwrap())).unwrap(),
            )
            .unwrap();
            assert_eq!(reading.metadata.kind, ReadingKind::Quote);
            assert!(reading.body.contains("A distinct quotation."));
            assert!(!reading.body.contains("First paragraph."));
            assert!(list_highlights(&library, response.id.as_deref().unwrap())
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn mismatched_or_unknown_quote_context_remains_an_article_highlight() {
        for context in [
            json!({"version": 1, "selected_text": "Different text.", "enclosing_element": "q"}),
            json!({"version": 2, "selected_text": "Selected ordinary passage.", "enclosing_element": "q"}),
        ] {
            let temporary = TempDir::new().unwrap();
            let library = LibraryRoot::new(temporary.path()).unwrap();
            let response = handle_in_library(
                request(message(
                    "https://example.com/article",
                    "Selected ordinary passage.",
                    Some(context),
                )),
                &library,
            )
            .unwrap();
            assert!(response.ok);
            assert_eq!(
                serde_json::to_value(&response).unwrap()["selection_kind"],
                "highlight"
            );
            let id = response.id.unwrap();
            assert_eq!(list_highlights(&library, &id).unwrap().len(), 1);
        }
    }

    #[test]
    fn selection_preserves_existing_article_body_state_and_assets() {
        let temporary = TempDir::new().unwrap();
        let library = LibraryRoot::new(temporary.path()).unwrap();
        let url = "https://example.com/existing";
        let image_bytes = b"existing captured image";
        let mut original = message(url, "Selected ordinary passage.", None);
        original["images"] = json!([{
            "url": "https://example.com/preview.png", "content_type": "image/png",
            "data_base64": base64::engine::general_purpose::STANDARD.encode(image_bytes),
        }]);
        original["preview_url"] = json!("https://example.com/preview.png");
        let first = handle_in_library(request(original), &library).unwrap();
        assert!(first.ok);
        let id = first.id.unwrap();
        let article = library.article_path(&id);
        let mut reading = parse_reading(&fs::read_to_string(&article).unwrap()).unwrap();
        reading.metadata.tags = vec!["kept-tag".into()];
        reading.metadata.favorite = true;
        fs::write(&article, render_reading(&reading).unwrap()).unwrap();
        let before = parse_reading(&fs::read_to_string(&article).unwrap()).unwrap();
        let asset = library
            .reading_dir(&id)
            .join(reading.metadata.preview_asset.as_ref().unwrap());
        let mut replacement = message(url, "First paragraph.", None);
        replacement["markdown"] =
            json!("Replacement full-page body must not overwrite the saved article.");
        replacement["metadata"]["title"] = json!("Replacement title");
        let second = handle_in_library(request(replacement), &library).unwrap();
        assert!(
            second.ok,
            "adding a new highlight to an existing article succeeds"
        );
        assert_eq!(second.id.as_deref(), Some(id.as_str()));
        let after = parse_reading(&fs::read_to_string(article).unwrap()).unwrap();
        assert_eq!(after.body.as_bytes(), before.body.as_bytes());
        let mut before_metadata = serde_json::to_value(&before.metadata).unwrap();
        let mut after_metadata = serde_json::to_value(&after.metadata).unwrap();
        // A new featured highlight changes its pointer; every prior metadata
        // field, including title, source hash, user state, and asset roles, stays.
        before_metadata
            .as_object_mut()
            .unwrap()
            .remove("featured_highlight_id");
        after_metadata
            .as_object_mut()
            .unwrap()
            .remove("featured_highlight_id");
        assert_eq!(after_metadata, before_metadata);
        assert_eq!(fs::read(asset).unwrap(), image_bytes);
        assert_eq!(list_highlights(&library, &id).unwrap().len(), 2);
    }

    #[test]
    fn lightweight_link_can_receive_full_capture_when_selection_is_saved() {
        let temporary = TempDir::new().unwrap();
        let library = LibraryRoot::new(temporary.path()).unwrap();
        let url = "https://example.com/link-upgrade";
        import_link(&library, url).unwrap();
        let response = handle_in_library(
            request(message(url, "Selected ordinary passage.", None)),
            &library,
        )
        .unwrap();
        assert!(response.ok);
        let id = response.id.unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&id)).unwrap()).unwrap();
        assert!(!reading.metadata.lightweight);
        assert!(reading.body.contains("First paragraph."));
        assert_eq!(list_highlights(&library, &id).unwrap().len(), 1);
    }

    #[test]
    fn missing_page_extraction_still_saves_a_lightweight_link_and_highlight() {
        let temporary = TempDir::new().unwrap();
        let library = LibraryRoot::new(temporary.path()).unwrap();
        let url = "https://example.com/unextractable";
        let mut value = message(url, "Selected ordinary passage.", None);
        value["markdown"] = json!("");
        let response = handle_in_library(request(value), &library).unwrap();
        assert!(response.ok);
        let id = response.id.unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&id)).unwrap()).unwrap();
        assert!(reading.metadata.lightweight);
        assert_eq!(list_highlights(&library, &id).unwrap().len(), 1);
    }

    #[test]
    fn untrusted_selection_evidence_is_bounded_before_library_resolution() {
        for mutation in [
            "selection",
            "context",
            "element",
            "product",
            "protocol",
            "kind",
        ] {
            let mut value = message(
                "https://example.com/article",
                "Selected ordinary passage.",
                None,
            );
            match mutation {
                "selection" => {
                    value["selected_text"] = json!("x".repeat(MAX_SELECTED_TEXT_BYTES + 1))
                }
                "context" => {
                    value["selection_context"] = json!({"version": 1, "selected_text": "x".repeat(MAX_SELECTED_TEXT_BYTES + 1)})
                }
                "element" => {
                    value["selection_context"] = json!({"version": 1, "selected_text": "Selected ordinary passage.", "enclosing_element": "x".repeat(MAX_CONTEXT_ELEMENT_BYTES + 1)})
                }
                "product" => {
                    value["metadata"]["product_html"] =
                        json!("x".repeat(oia_core::product::MAX_BROWSER_PRODUCT_HTML_BYTES + 1))
                }
                "protocol" => value["protocol_version"] = json!(3),
                "kind" => value["metadata"]["kind"] = json!("video"),
                _ => unreachable!(),
            }
            let response = handle(request(value)).unwrap();
            assert!(!response.ok, "{mutation}");
            assert_eq!(
                response.error.as_deref(),
                Some("invalid_request"),
                "{mutation}"
            );
        }
    }
}
