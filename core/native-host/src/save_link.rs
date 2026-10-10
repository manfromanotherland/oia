// SPDX-License-Identifier: MIT

use anyhow::Result;
use oia_core::{
    save_link_capture, save_special_url, LibraryRoot, SaveError, SaveLinkInput, UrlSaveRequest,
};

use crate::{
    protocol::{SaveLinkRequest, SaveResponse, PROTOCOL_VERSION},
    save::{decode_images, find_library_path, response_for_outcome, response_for_source_error},
};

pub fn handle(request: SaveLinkRequest) -> Result<SaveResponse> {
    if request.protocol_version != PROTOCOL_VERSION {
        return Ok(SaveResponse::error(
            "invalid_request",
            &format!("unsupported protocol_version: {}", request.protocol_version),
        ));
    }
    if request.action != "save_link" {
        return Ok(SaveResponse::error(
            "invalid_request",
            &format!("unknown action: {}", request.action),
        ));
    }
    if request.metadata.kind != oia_core::ReadingKind::Article {
        return Ok(SaveResponse::error(
            "invalid_request",
            "save_link requires article metadata",
        ));
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
    let library = LibraryRoot::new(&library_path)?;

    handle_in_library_with_source_resolver(request, &library, save_special_url)
}

fn handle_in_library_with_source_resolver(
    request: SaveLinkRequest,
    library: &LibraryRoot,
    resolve_source: impl FnOnce(
        &LibraryRoot,
        &UrlSaveRequest,
    ) -> Result<Option<oia_core::SaveOutcome>, oia_core::SaveUrlError>,
) -> Result<SaveResponse> {
    if request
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
    let special_request = UrlSaveRequest {
        url: request.metadata.url.clone(),
        title_hint: Some(request.metadata.title.clone()),
        saved_at: Some(request.metadata.saved_at.clone()),
    };
    match resolve_source(library, &special_request) {
        Ok(Some(outcome)) => return Ok(response_for_outcome(outcome)),
        Ok(None) => {}
        Err(error) => return response_for_source_error(error),
    }

    let product = (request.metadata.kind == oia_core::ReadingKind::Article)
        .then(|| {
            request
                .metadata
                .product_html
                .as_deref()
                .and_then(|html| oia_core::parse_product_metadata(html, &request.metadata.url))
        })
        .flatten();
    let images = decode_images(&request.images);

    let outcome = match save_link_capture(
        library,
        SaveLinkInput {
            url: request.metadata.url,
            canonical_url: request.metadata.canonical_url,
            title: request.metadata.title,
            author: request.metadata.author,
            site: request.metadata.site,
            saved_at: request.metadata.saved_at,
            images,
            preview_url: request.preview_url,
            favicon_url: request.favicon_url,
            theme_color: request.metadata.theme_color,
            product,
            excerpt: request.metadata.excerpt,
            lang: request.metadata.lang,
        },
    ) {
        Ok(outcome) => outcome,
        Err(SaveError::InvalidRequest(message)) => {
            return Ok(SaveResponse::error("invalid_request", &message));
        }
        Err(SaveError::Storage(error)) => return Err(error),
    };

    Ok(response_for_outcome(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oia_core::SaveUrlError;

    #[test]
    fn url_only_save_remains_failure_strict_when_source_retrieval_fails() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let request: SaveLinkRequest = serde_json::from_value(serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "action": "save_link",
            "metadata": {
                "kind": "article",
                "url": "https://x.com/example/status/42",
                "canonical_url": "https://x.com/example/status/42",
                "title": "X post",
                "saved_at": "2026-09-23T12:00:00.000Z"
            },
            "images": []
        }))
        .unwrap();

        let response = handle_in_library_with_source_resolver(request, &library, |_, _| {
            Err(SaveUrlError::Retrieval("offline".into()))
        })
        .unwrap();

        assert!(!response.ok);
        assert_eq!(response.error.as_deref(), Some("source_unavailable"));
        assert!(!library.articles_dir().exists());
    }
    fn product_request(html: &str) -> SaveLinkRequest {
        serde_json::from_value(serde_json::json!({"protocol_version":PROTOCOL_VERSION,"action":"save_link","metadata":{"kind":"article","url":"https://shop.example/lamp","canonical_url":"https://shop.example/lamp","title":"Lamp","saved_at":"2026-10-10T12:00:00.000Z","product_html":html},"images":[]})).unwrap()
    }
    #[test]
    fn save_link_classifies_live_structured_product_and_enriches_existing_link() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let existing = oia_core::import_link(&library, "https://shop.example/lamp").unwrap();
        let original = oia_core::parse_reading(
            &std::fs::read_to_string(library.article_path(&existing.id)).unwrap(),
        )
        .unwrap();
        let request = product_request(
            r#"<script type="application/ld+json">{"@type":"Product","offers":{"price":140,"priceCurrency":"EUR"}}</script>"#,
        );
        let response =
            handle_in_library_with_source_resolver(request, &library, |_, _| Ok(None)).unwrap();
        assert!(response.ok);
        let reading = oia_core::parse_reading(
            &std::fs::read_to_string(library.article_path(&existing.id)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            reading.metadata.product.unwrap().price.as_deref(),
            Some("140")
        );
        assert_eq!(reading.body, original.body);
        assert_eq!(reading.metadata.saved_at, original.metadata.saved_at);
        assert!(reading.metadata.lightweight);
    }
    #[test]
    fn rejects_oversized_live_evidence_before_any_source_retrieval() {
        let dir = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let request =
            product_request(&"x".repeat(oia_core::product::MAX_BROWSER_PRODUCT_HTML_BYTES + 1));
        let response = handle_in_library_with_source_resolver(request, &library, |_, _| {
            panic!("must reject before retrieval")
        })
        .unwrap();
        assert_eq!(response.error.as_deref(), Some("invalid_request"));
        assert!(!library.articles_dir().exists());
    }
}
