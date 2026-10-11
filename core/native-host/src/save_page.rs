// SPDX-License-Identifier: MIT

use anyhow::Result;
use oia_core::{
    classify_page_capture, save_special_url, LibraryRoot, PageCaptureKind, ReadingKind, SaveError,
    SaveOutcome, SaveUrlError, UrlSaveRequest,
};

use crate::{
    protocol::{SaveRequest, SaveResponse, PROTOCOL_VERSION},
    save::{
        find_library_path, persist_capture_in_library, response_for_outcome,
        response_for_source_error,
    },
};

/// Unified toolbar save. The browser sends extraction observations, never a
/// choice between an article and a lightweight link.
pub fn handle(request: SaveRequest) -> Result<SaveResponse> {
    if let Some(response) = validate_request(&request) {
        return Ok(response);
    }
    let library_path = match find_library_path() {
        Ok(path) => path,
        Err(_) => {
            return Ok(SaveResponse::error(
                "library_not_configured",
                "No library folder has been set. Open the Óia app to configure one.",
            ));
        }
    };
    let library = LibraryRoot::new(library_path)?;
    handle_in_library_with_source_resolver(request, &library, save_special_url)
}

fn validate_request(request: &SaveRequest) -> Option<SaveResponse> {
    let invalid = |message: &str| Some(SaveResponse::error("invalid_request", message));
    if request.protocol_version != PROTOCOL_VERSION || request.action != "save_page" {
        return invalid("unsupported automatic page save request");
    }
    if request.metadata.kind != ReadingKind::Article || request.metadata.media_url.is_some() {
        return invalid("automatic page saves require page metadata");
    }
    if request
        .metadata
        .product_html
        .as_ref()
        .is_some_and(|html| html.len() > oia_core::product::MAX_BROWSER_PRODUCT_HTML_BYTES)
    {
        return invalid("product metadata evidence exceeded 512 KiB");
    }
    if request
        .page_content
        .as_ref()
        .is_some_and(|evidence| !evidence.is_valid())
    {
        return invalid("page content evidence is invalid or exceeded its size limit");
    }
    None
}

fn handle_in_library_with_source_resolver(
    request: SaveRequest,
    library: &LibraryRoot,
    resolve_source: impl FnOnce(
        &LibraryRoot,
        &UrlSaveRequest,
    ) -> Result<Option<SaveOutcome>, SaveUrlError>,
) -> Result<SaveResponse> {
    if let Some(response) = validate_request(&request) {
        return Ok(response);
    }
    let source_request = UrlSaveRequest {
        url: request.metadata.url.clone(),
        title_hint: Some(request.metadata.title.clone()),
        saved_at: Some(request.metadata.saved_at.clone()),
    };
    let source_fallback = match resolve_source(library, &source_request) {
        Ok(Some(outcome)) => return Ok(response_for_outcome(outcome)),
        Ok(None) => false,
        Err(error @ SaveUrlError::Retrieval(_))
        | Err(error @ SaveUrlError::Save(SaveError::InvalidRequest(_))) => {
            if request.markdown.trim().is_empty() {
                return response_for_source_error(error);
            }
            // A recognized provider's failed retrieval must not discard usable
            // live content. An empty capture still retains URL-only strictness.
            true
        }
        Err(error) => return response_for_source_error(error),
    };
    let page_kind = if source_fallback {
        PageCaptureKind::Article
    } else {
        classify_page_capture(
            &request.metadata.url,
            request.metadata.product_html.as_deref().unwrap_or(""),
            &request.markdown,
            request.page_content.as_ref(),
        )
    };
    persist_capture_in_library(request, library, page_kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oia_core::{parse_reading, PageCaptureEvidence};
    use serde_json::json;

    fn request(url: &str) -> SaveRequest {
        serde_json::from_value(json!({
            "protocol_version": PROTOCOL_VERSION,
            "action": "save_page",
            "metadata": { "kind": "article", "url": url, "canonical_url": url,
                "title": "Live page", "saved_at": "2026-10-11T12:00:00.000Z" },
            "markdown": "Captured live body.", "images": [],
            "page_content": { "text_characters": 1000, "link_characters": 20,
                "paragraph_characters": [450, 450] },
        }))
        .unwrap()
    }

    fn read(library: &LibraryRoot, response: &SaveResponse) -> oia_core::Reading {
        parse_reading(
            &std::fs::read_to_string(library.article_path(response.id.as_ref().unwrap())).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn automatic_save_writes_prose_as_an_article_and_empty_or_weak_extraction_as_links() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let response = handle_in_library_with_source_resolver(
            request("https://example.com/article"),
            &library,
            |_, _| Ok(None),
        )
        .unwrap();
        let article = read(&library, &response);
        assert!(!article.metadata.lightweight);
        assert_eq!(article.body, "Captured live body.\n");
        for (url, markdown, evidence) in [
            ("https://example.com/empty", "", None),
            (
                "https://example.com/home",
                "Homepage labels",
                Some(PageCaptureEvidence {
                    text_characters: 800,
                    link_characters: 50,
                    paragraph_characters: vec![40; 20],
                }),
            ),
        ] {
            let mut capture = request(url);
            capture.markdown = markdown.into();
            capture.page_content = evidence;
            let response =
                handle_in_library_with_source_resolver(capture, &library, |_, _| Ok(None)).unwrap();
            let link = read(&library, &response);
            assert!(link.metadata.lightweight);
            assert_eq!(link.body, format!("[Open link](<{url}>)\n"));
        }
    }

    #[test]
    fn automatic_catalog_save_preserves_typed_metadata_and_never_replaces_a_full_capture() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://example.com/movie";
        let mut movie = request(url);
        movie.metadata.product_html = Some(format!(
            r#"<script type="application/ld+json">{{"@type":"Movie","url":"{url}","name":"Movie title"}}</script>"#,
        ));
        let response =
            handle_in_library_with_source_resolver(movie, &library, |_, _| Ok(None)).unwrap();
        let reading = read(&library, &response);
        assert!(reading.metadata.lightweight);
        assert_eq!(reading.metadata.link_category.as_deref(), Some("movie"));

        let full_url = "https://example.com/full";
        let full = crate::save::persist_capture_in_library(
            request(full_url),
            &library,
            PageCaptureKind::Article,
        )
        .unwrap();
        let mut later_link = request(full_url);
        later_link.markdown.clear();
        let duplicate =
            handle_in_library_with_source_resolver(later_link, &library, |_, _| Ok(None)).unwrap();
        assert_eq!(duplicate.error.as_deref(), Some("duplicate"));
        let reading = read(&library, &full);
        assert!(!reading.metadata.lightweight);
        assert_eq!(reading.body, "Captured live body.\n");
    }

    #[test]
    fn source_retrieval_has_priority_and_live_content_remains_the_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let source = handle_in_library_with_source_resolver(
            request("https://x.com/example/status/42"),
            &library,
            |_, _| {
                Ok(Some(SaveOutcome {
                    id: "resolved-source".into(),
                    path: "resolved.md".into(),
                    disposition: oia_core::SaveDisposition::Saved,
                }))
            },
        )
        .unwrap();
        assert_eq!(source.id.as_deref(), Some("resolved-source"));

        for (id, error) in [
            (43, SaveUrlError::Retrieval("offline".into())),
            (
                44,
                SaveUrlError::Save(SaveError::InvalidRequest("bad payload".into())),
            ),
        ] {
            let mut capture = request(&format!("https://x.com/example/status/{id}"));
            capture.page_content = None;
            let response =
                handle_in_library_with_source_resolver(capture, &library, |_, _| Err(error))
                    .unwrap();
            assert!(response.ok);
            assert!(!read(&library, &response).metadata.lightweight);
        }
        let mut empty = request("https://x.com/example/status/45");
        empty.markdown.clear();
        let failed = handle_in_library_with_source_resolver(empty, &library, |_, _| {
            Err(SaveUrlError::Retrieval("offline".into()))
        })
        .unwrap();
        assert_eq!(failed.error.as_deref(), Some("source_unavailable"));
        assert!(failed.id.is_none());
    }

    #[test]
    fn automatic_save_rejects_media_and_unbounded_or_inconsistent_evidence() {
        let mut media = request("https://example.com/page");
        media.metadata.kind = ReadingKind::Image;
        assert!(validate_request(&media).is_some());
        let mut oversized = request("https://example.com/page");
        oversized
            .page_content
            .as_mut()
            .unwrap()
            .paragraph_characters = vec![40; oia_core::MAX_PAGE_CAPTURE_PARAGRAPHS + 1];
        assert!(validate_request(&oversized).is_some());
        let mut inconsistent = request("https://example.com/page");
        inconsistent.page_content.as_mut().unwrap().link_characters = 1001;
        assert!(validate_request(&inconsistent).is_some());
    }
}
