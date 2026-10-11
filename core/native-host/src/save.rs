// SPDX-License-Identifier: MIT

use std::path::PathBuf;

use anyhow::{bail, Result};
use base64::Engine;
use oia_core::{
    save_capture, save_link_capture, save_special_url, ImageBytes, LibraryRoot, PageCaptureKind,
    ReadingKind, SaveDisposition, SaveError, SaveInput, SaveLinkInput, SaveUrlError,
    UrlSaveRequest,
};

use crate::protocol::{SaveRequest, SaveResponse, PROTOCOL_VERSION};

pub fn handle(req: SaveRequest) -> Result<SaveResponse> {
    if req.protocol_version != PROTOCOL_VERSION {
        return Ok(SaveResponse::error(
            "invalid_request",
            &format!("unsupported protocol_version: {}", req.protocol_version),
        ));
    }
    if req.action != "save" {
        return Ok(SaveResponse::error(
            "invalid_request",
            &format!("unknown action: {}", req.action),
        ));
    }
    if req.metadata.kind == ReadingKind::Video {
        return Ok(SaveResponse::error(
            "invalid_request",
            "browser video saves require the streaming video import",
        ));
    }

    let library_path = match find_library_path() {
        Ok(p) => p,
        Err(_) => {
            return Ok(SaveResponse::error(
                "library_not_configured",
                "No library folder has been set. Open the Óia app to configure one.",
            ))
        }
    };
    let library = LibraryRoot::new(&library_path)?;

    handle_in_library_with_source_resolver(req, &library, save_special_url)
}

fn handle_in_library_with_source_resolver(
    req: SaveRequest,
    library: &LibraryRoot,
    resolve_source: impl FnOnce(
        &LibraryRoot,
        &UrlSaveRequest,
    ) -> Result<Option<oia_core::SaveOutcome>, SaveUrlError>,
) -> Result<SaveResponse> {
    if req
        .metadata
        .product_html
        .as_ref()
        .is_some_and(|html| html.len() > oia_core::product::MAX_BROWSER_PRODUCT_HTML_BYTES)
    {
        return Ok(SaveResponse::error(
            "invalid_request",
            "product metadata evidence exceeded 512 KiB",
        ));
    }
    if req.metadata.kind == ReadingKind::Article {
        let special_request = UrlSaveRequest {
            url: req.metadata.url.clone(),
            title_hint: Some(req.metadata.title.clone()),
            canonical_url_hint: Some(req.metadata.canonical_url.clone()),
            site_hint: req.metadata.site.clone(),
            saved_at: Some(req.metadata.saved_at.clone()),
        };
        match resolve_source(library, &special_request) {
            Ok(Some(outcome)) => return Ok(response_for_outcome(outcome)),
            Ok(None) => {}
            Err(SaveUrlError::Retrieval(_))
            | Err(SaveUrlError::Save(SaveError::InvalidRequest(_))) => {}
            Err(error) => return response_for_source_error(error),
        }
    }

    persist_capture_in_library(req, library, PageCaptureKind::Article)
}

pub(crate) fn persist_capture_in_library(
    req: SaveRequest,
    library: &LibraryRoot,
    page_kind: PageCaptureKind,
) -> Result<SaveResponse> {
    // Decode the image bytes the extension captured. An image whose base64 won't
    // decode is skipped, so its URL stays in the Markdown as a placeholder.
    let product = (req.metadata.kind == oia_core::ReadingKind::Article)
        .then(|| {
            req.metadata
                .product_html
                .as_deref()
                .and_then(|html| oia_core::parse_product_metadata(html, &req.metadata.url))
        })
        .flatten();
    let detected = (req.metadata.kind == ReadingKind::Article)
        .then(|| {
            oia_core::parse_link_metadata(
                req.metadata.product_html.as_deref().unwrap_or(""),
                &req.metadata.url,
            )
        })
        .flatten();
    let images = decode_images(&req.images);
    let preview_url = detected_preview_url(detected.as_ref(), &images, req.preview_url);

    let saved = match page_kind {
        PageCaptureKind::Link => save_link_capture(
            library,
            SaveLinkInput {
                url: req.metadata.url,
                canonical_url: req.metadata.canonical_url,
                title: detected
                    .as_ref()
                    .and_then(|value| value.title.clone())
                    .unwrap_or(req.metadata.title),
                author: req.metadata.author,
                site: req.metadata.site,
                saved_at: req.metadata.saved_at,
                images,
                preview_url,
                favicon_url: req.favicon_url,
                theme_color: req.metadata.theme_color,
                product,
                link_category: detected.map(|value| value.category),
                excerpt: req.metadata.excerpt,
                lang: req.metadata.lang,
            },
        ),
        PageCaptureKind::Article => save_capture(
            library,
            SaveInput {
                quote_identity_markdown: None,
                kind: req.metadata.kind,
                lightweight: false,
                url: req.metadata.url,
                media_url: req.metadata.media_url,
                canonical_url: req.metadata.canonical_url,
                title: detected
                    .as_ref()
                    .and_then(|value| value.title.clone())
                    .unwrap_or(req.metadata.title),
                author: req.metadata.author,
                site: req.metadata.site,
                saved_at: req.metadata.saved_at,
                markdown: req.markdown,
                images,
                preview_url,
                favicon_url: req.favicon_url,
                theme_color: req.metadata.theme_color,
                product,
                link_category: detected.map(|value| value.category),
                excerpt: req.metadata.excerpt,
                word_count: req.metadata.word_count,
                lang: req.metadata.lang,
            },
        ),
    };
    let outcome = match saved {
        Ok(outcome) => outcome,
        Err(SaveError::InvalidRequest(message)) => {
            return Ok(SaveResponse::error("invalid_request", &message));
        }
        Err(SaveError::Storage(error)) => return Err(error),
    };

    Ok(response_for_outcome(outcome))
}

pub(crate) fn response_for_outcome(outcome: oia_core::SaveOutcome) -> SaveResponse {
    if outcome.disposition == SaveDisposition::Duplicate {
        return SaveResponse::error(
            "duplicate",
            &format!("This reading already exists (id: {})", outcome.id),
        );
    }

    SaveResponse::success(outcome.id, outcome.path)
}

pub(crate) fn response_for_source_error(error: SaveUrlError) -> Result<SaveResponse> {
    match error {
        error @ SaveUrlError::Retrieval(_) => Ok(SaveResponse::error(
            "source_unavailable",
            &error.to_string(),
        )),
        SaveUrlError::Save(SaveError::InvalidRequest(message)) => {
            Ok(SaveResponse::error("invalid_request", &message))
        }
        SaveUrlError::Save(SaveError::Storage(error)) | SaveUrlError::Storage(error) => Err(error),
    }
}

pub(crate) fn decode_images(images: &[crate::protocol::RequestImage]) -> Vec<ImageBytes> {
    images
        .iter()
        .filter_map(|image| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&image.data_base64)
                .ok()?;
            Some(ImageBytes {
                url: image.url.clone(),
                content_type: image.content_type.clone(),
                bytes,
            })
        })
        .collect()
}

/// The exact main entity image can replace the social preview only when captured.
pub(crate) fn detected_preview_url(
    detected: Option<&oia_core::DetectedLinkMetadata>,
    images: &[ImageBytes],
    fallback: Option<String>,
) -> Option<String> {
    detected
        .and_then(|value| value.image_url.as_ref())
        .filter(|url| images.iter().any(|image| &image.url == *url))
        .cloned()
        .or(fallback)
}

/// Classify an anyhow error into a protocol error code + message.
pub fn classify_error(e: &anyhow::Error) -> (&'static str, String) {
    ("io_error", e.to_string())
}

pub(crate) fn find_library_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CUTTINGS_LIBRARY") {
        return Ok(PathBuf::from(path));
    }

    let home = std::env::var("HOME")?;
    let config_file = PathBuf::from(home).join(".config/cuttings/library");
    if config_file.is_file() {
        let path = std::fs::read_to_string(config_file)?.trim().to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    bail!("library_not_configured")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oia_core::SaveUrlError;

    #[test]
    fn full_capture_persists_cultural_category_and_exact_captured_cover() {
        for (entity_type, category) in [
            ("Book", "book"),
            ("MusicAlbum", "music"),
            ("Movie", "movie"),
        ] {
            let temporary = tempfile::tempdir().unwrap();
            let library = LibraryRoot::new(temporary.path()).unwrap();
            let mut request = captured_article_request("https://example.com/work");
            request.metadata.product_html = Some(format!(
                r#"<script type="application/ld+json">{{"@type":"{entity_type}","name":"Exact work","url":"https://example.com/work","image":"https://example.com/cover.png"}}</script>"#
            ));
            request.preview_url = Some("https://example.com/social.png".into());
            request.images = serde_json::from_value(serde_json::json!([
                {"url":"https://example.com/social.png","content_type":"image/png","data_base64":"c29jaWFs"},
                {"url":"https://example.com/cover.png","content_type":"image/png","data_base64":"Y292ZXI="},
            ])).unwrap();
            let response =
                handle_in_library_with_source_resolver(request, &library, |_, _| Ok(None)).unwrap();
            assert!(response.ok);
            let id = response.id.unwrap();
            let reading = oia_core::parse_reading(
                &std::fs::read_to_string(library.article_path(&id)).unwrap(),
            )
            .unwrap();
            assert_eq!(reading.metadata.link_category.as_deref(), Some(category));
            assert_eq!(reading.metadata.title, "Exact work");
            assert_eq!(
                std::fs::read(
                    library
                        .reading_dir(&id)
                        .join(reading.metadata.preview_asset.unwrap())
                )
                .unwrap(),
                b"cover"
            );
        }
    }

    #[test]
    fn unavailable_typed_artwork_retains_captured_social_preview() {
        let detected = oia_core::parse_link_metadata(
            r#"<meta property="og:type" content="book"><meta property="og:image" content="https://example.com/cover.png">"#,
            "https://example.com/book",
        );
        let images = [ImageBytes {
            url: "https://example.com/social.png".into(),
            content_type: "image/png".into(),
            bytes: b"social".to_vec(),
        }];
        assert_eq!(
            detected_preview_url(
                detected.as_ref(),
                &images,
                Some("https://example.com/social.png".into())
            )
            .as_deref(),
            Some("https://example.com/social.png")
        );
    }

    #[test]
    fn standalone_quote_does_not_inherit_source_book_category_or_title() {
        let temporary = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(temporary.path()).unwrap();
        let mut request = captured_article_request("https://example.com/book");
        request.metadata.kind = ReadingKind::Quote;
        request.metadata.product_html = Some(
            r#"<script type="application/ld+json">{"@type":"Book","name":"Book source"}</script>"#
                .into(),
        );
        request.metadata.title = "Selected quotation".into();
        request.markdown = "A selected quotation.".into();
        let response = handle_in_library_with_source_resolver(request, &library, |_, _| {
            panic!("standalone capture must not retrieve a source")
        })
        .unwrap();
        assert!(response.ok);
        let reading = oia_core::parse_reading(
            &std::fs::read_to_string(library.article_path(&response.id.unwrap())).unwrap(),
        )
        .unwrap();
        assert!(reading.metadata.link_category.is_none());
        assert!(reading.metadata.product.is_none());
        assert_eq!(reading.metadata.title, "Selected quotation");
    }

    fn captured_article_request(url: &str) -> SaveRequest {
        serde_json::from_value(serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "action": "save",
            "metadata": {
                "kind": "article",
                "url": url,
                "canonical_url": url,
                "title": "Captured post",
                "site": "X",
                "saved_at": "2026-09-23T12:00:00.000Z"
            },
            "markdown": "The complete post captured from the live page.",
            "images": []
        }))
        .unwrap()
    }

    #[test]
    fn full_article_falls_back_to_browser_capture_when_source_retrieval_fails() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let request = captured_article_request("https://x.com/example/status/42");

        let response = handle_in_library_with_source_resolver(request, &library, |_, _| {
            Err(SaveUrlError::Retrieval("offline".into()))
        })
        .unwrap();

        assert!(response.ok);
        let article = std::fs::read_to_string(
            directory
                .path()
                .join(response.path.expect("saved article path")),
        )
        .unwrap();
        let reading = oia_core::parse_reading(&article).unwrap();
        assert_eq!(
            reading.body,
            "The complete post captured from the live page.\n"
        );
        assert!(reading.metadata.source_profile.is_none());
    }

    #[test]
    fn full_article_falls_back_to_browser_capture_when_source_payload_is_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let request = captured_article_request("https://x.com/example/status/43");

        let response = handle_in_library_with_source_resolver(request, &library, |_, _| {
            Err(SaveUrlError::Save(SaveError::InvalidRequest(
                "invalid source payload".into(),
            )))
        })
        .unwrap();

        assert!(response.ok);
        let article = std::fs::read_to_string(
            directory
                .path()
                .join(response.path.expect("saved article path")),
        )
        .unwrap();
        let reading = oia_core::parse_reading(&article).unwrap();
        assert_eq!(
            reading.body,
            "The complete post captured from the live page.\n"
        );
        assert!(reading.metadata.source_profile.is_none());
    }

    #[test]
    fn source_retrieval_failure_is_reported_as_source_unavailable() {
        let response =
            response_for_source_error(SaveUrlError::Retrieval("offline".into())).unwrap();

        assert_eq!(response.error.as_deref(), Some("source_unavailable"));
    }

    #[test]
    fn source_invalid_request_is_reported_as_invalid_request() {
        let response = response_for_source_error(SaveUrlError::Save(SaveError::InvalidRequest(
            "bad source capture".into(),
        )))
        .unwrap();

        assert_eq!(response.error.as_deref(), Some("invalid_request"));
    }

    #[test]
    fn source_storage_failure_remains_a_host_io_error() {
        let result = response_for_source_error(SaveUrlError::Storage(anyhow::anyhow!("disk full")));

        assert_eq!(result.unwrap_err().to_string(), "disk full");
    }
}
