// SPDX-License-Identifier: MIT

use anyhow::Result;
use oia_core::{
    classify_page_capture, LibraryRoot, PageCaptureKind, ReadingKind, SaveError, SaveOutcome,
    SaveUrlError, UrlSaveRequest,
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
    handle_in_library_with_source_resolver(request, &library, crate::source_resolver::resolve)
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
        canonical_url_hint: Some(request.metadata.canonical_url.clone()),
        site_hint: request.metadata.site.clone(),
        saved_at: Some(request.metadata.saved_at.clone()),
    };
    let source_fallback = match resolve_source(library, &source_request) {
        Ok(Some(outcome)) => return Ok(response_for_outcome(outcome)),
        Ok(None) => false,
        Err(error @ SaveUrlError::Retrieval(_))
        | Err(error @ SaveUrlError::Save(SaveError::InvalidRequest(_))) => {
            if request.markdown.trim().is_empty()
                || oia_core::instagram::is_post_url(&source_request.url)
            {
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

    #[test]
    fn instagram_page_never_saves_article_text_when_media_is_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let response = handle_in_library_with_source_resolver(
            request("https://www.instagram.com/p/DdlVpikk5Gj/?img_index=2"),
            &library,
            |_, _| Err(SaveUrlError::Retrieval("selected media unavailable".into())),
        )
        .unwrap();
        assert_eq!(response.error.as_deref(), Some("source_unavailable"));
        assert!(response.id.is_none());
        assert_eq!(oia_core::scan_library(&library).unwrap().len(), 0);
    }

    #[test]
    fn instagram_toolbar_save_persists_selected_media_and_deduplicates() {
        use base64::Engine;
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAaX2RUAAAAAASUVORK5CYII="
        ).unwrap();
        let resolver = |instagram: &oia_core::instagram::InstagramRequest| {
            assert_eq!(instagram.slide, 2);
            assert_eq!(
                instagram.origin,
                "https://www.instagram.com/p/DdlVpikk5Gj/?img_index=2"
            );
            let directory = tempfile::tempdir()?;
            let name = "payload.jpg".to_string();
            std::fs::write(directory.path().join(&name), &bytes)?;
            Ok(oia_core::instagram::DownloadedMedia { directory, name })
        };
        for duplicate in [false, true] {
            let mut capture =
                request("https://www.instagram.com/p/DdlVpikk5Gj/?img_index=2&stkn=tracking");
            // A canonical URL may omit the selected slide. The original URL owns it.
            capture.metadata.canonical_url = "https://www.instagram.com/p/DdlVpikk5Gj/".into();
            let response =
                handle_in_library_with_source_resolver(capture, &library, |library, request| {
                    crate::source_resolver::resolve_with_instagram(library, request, &resolver)
                })
                .unwrap();
            assert_eq!(response.error.as_deref(), duplicate.then_some("duplicate"));
            if duplicate {
                continue;
            }
            assert!(response.ok, "{response:?}");
            let reading = read(&library, &response);
            assert_eq!(reading.metadata.kind, ReadingKind::Image);
            assert!(!reading.metadata.lightweight);
            assert_eq!(
                reading.metadata.url,
                "https://instagram.com/p/DdlVpikk5Gj?img_index=2"
            );
            assert_eq!(reading.metadata.saved_at, "2026-10-11T12:00:00.000Z");
            let media = reading.metadata.media_url.unwrap();
            let asset = media.strip_prefix("cuttings-asset:").unwrap();
            assert_eq!(
                std::fs::read(
                    library
                        .reading_dir(response.id.as_ref().unwrap())
                        .join(asset)
                )
                .unwrap(),
                bytes
            );
            assert!(!reading.body.contains("Captured live body"));
        }
        assert_eq!(oia_core::scan_library(&library).unwrap().len(), 1);
    }

    #[test]
    fn instagram_toolbar_rejects_invalid_slide_and_incomplete_media() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        for url in [
            "https://instagram.com/reel/abc/?img_index=0",
            "https://instagram.com/reel/abc/",
        ] {
            let response = handle_in_library_with_source_resolver(
                request(url),
                &library,
                |library, request| {
                    crate::source_resolver::resolve_with_instagram(library, request, &|instagram| {
                        assert_eq!(instagram.slide, 1);
                        let directory = tempfile::tempdir()?;
                        let name = "payload.mp4".to_string();
                        std::fs::write(directory.path().join(&name), b"incomplete movie")?;
                        Ok(oia_core::instagram::DownloadedMedia { directory, name })
                    })
                },
            )
            .unwrap();
            assert!(!response.ok);
            assert!(response.id.is_none());
        }
        assert_eq!(oia_core::scan_library(&library).unwrap().len(), 0);
    }

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
    fn harvard_save_uses_main_book_author_and_category_instead_of_recommendations() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://www.hup.harvard.edu/books/9780674627512";
        let mut capture = request(url);
        capture.metadata.author = Some("Blair Kamin".into());
        capture.metadata.product_html =
            Some(include_str!("../../core/tests/fixtures/harvard-book.html").into());
        let response =
            handle_in_library_with_source_resolver(capture, &library, |_, _| Ok(None)).unwrap();
        let reading = read(&library, &response);
        assert!(reading.metadata.lightweight);
        assert_eq!(reading.metadata.title, "Notes on the Synthesis of Form");
        assert_eq!(
            reading.metadata.author.as_deref(),
            Some("Christopher Alexander")
        );
        assert_eq!(reading.metadata.link_category.as_deref(), Some("book"));
        assert_eq!(
            reading.body,
            "[Open link](<https://hup.harvard.edu/books/9780674627512>)\n"
        );
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
    fn automatic_product_resave_enriches_an_existing_full_article_without_replacing_it() {
        let directory = tempfile::tempdir().unwrap();
        let library_path = directory.path().join("library");
        std::fs::create_dir(&library_path).unwrap();
        let library = LibraryRoot::new(library_path).unwrap();
        let url = "https://example.com/ronja-product";
        let original_response =
            handle_in_library_with_source_resolver(request(url), &library, |_, _| Ok(None))
                .unwrap();
        let id = original_response.id.as_ref().unwrap();
        let index = oia_core::open_index(&directory.path().join("index.db")).unwrap();
        oia_core::set_favorite(&library, &index, id, true).unwrap();
        oia_core::add_tag(&library, &index, id, "Keep tag").unwrap();
        let original = read(&library, &original_response);
        assert!(!original.metadata.lightweight);
        assert!(original.metadata.product.is_none());

        let product_capture = |url: &str| {
            let mut capture = request(url);
            capture.metadata.title = "Replacement product title".into();
            capture.markdown = "Replacement body must never be saved.".into();
            capture.metadata.product_html = Some(format!(
                r#"<script type="application/ld+json">{{"@type":"Product","url":"{url}","name":"Ronja","offers":{{"@type":"Offer","price":"135","priceCurrency":"EUR"}}}}</script>"#,
            ));
            capture
        };

        // Control proves the exact evidence is recognized by this save route.
        let control = handle_in_library_with_source_resolver(
            product_capture("https://example.com/fresh-product"),
            &library,
            |_, _| Ok(None),
        )
        .unwrap();
        assert!(read(&library, &control).metadata.product.is_some());

        let upgraded =
            handle_in_library_with_source_resolver(product_capture(url), &library, |_, _| Ok(None))
                .unwrap();
        assert!(upgraded.ok, "recognized product re-save: {upgraded:?}");
        assert_eq!(upgraded.id.as_ref(), Some(id));
        let after = read(&library, &original_response);
        let product = after.metadata.product.as_ref().unwrap();
        assert_eq!(product.price.as_deref(), Some("135"));
        assert_eq!(product.currency.as_deref(), Some("EUR"));
        let mut expected_metadata = original.metadata;
        expected_metadata.product = after.metadata.product.clone();
        assert_eq!(after.metadata, expected_metadata);
        assert_eq!(after.body, original.body);

        oia_core::rebuild(&index, &library).unwrap();
        let links = oia_core::list_readings(
            &index,
            &oia_core::ListOptions {
                view: oia_core::View::Links,
                ..Default::default()
            },
        )
        .unwrap();
        let row = links.iter().find(|row| &row.id == id).unwrap();
        assert!(row.is_link);
        assert!(!row.lightweight);
        assert!(row.product_json.is_some());
        let articles = oia_core::list_readings(
            &index,
            &oia_core::ListOptions {
                view: oia_core::View::Articles,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!articles.iter().any(|row| &row.id == id));
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
