// SPDX-License-Identifier: MIT

use crate::protocol::{InspectPageMetadataRequest, SaveResponse, PROTOCOL_VERSION};

const MAX_PAGE_URL_BYTES: usize = 8 * 1024;

/// Interpret browser evidence without opening a library or retrieving a URL.
pub fn handle(request: InspectPageMetadataRequest) -> SaveResponse {
    if request.protocol_version != PROTOCOL_VERSION || request.action != "inspect_page_metadata" {
        return SaveResponse::error("invalid_request", "unsupported metadata inspection request");
    }
    if request.product_html.len() > oia_core::product::MAX_BROWSER_PRODUCT_HTML_BYTES
        || request.url.len() > MAX_PAGE_URL_BYTES
    {
        return SaveResponse::error(
            "invalid_request",
            "metadata inspection exceeded its size limit",
        );
    }
    let valid_url = url::Url::parse(&request.url).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    });
    if !valid_url {
        return SaveResponse::error(
            "invalid_request",
            "metadata inspection requires an HTTP(S) URL",
        );
    }
    let mut response = SaveResponse::ack();
    response.preview_candidates = Some(
        oia_core::parse_link_metadata(&request.product_html, &request.url)
            .and_then(|detected| detected.image_url)
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(html: &str) -> InspectPageMetadataRequest {
        serde_json::from_value(json!({
            "protocol_version":4, "action":"inspect_page_metadata",
            "url":"https://example.com/book", "product_html":html,
        }))
        .unwrap()
    }

    #[test]
    fn inspection_selects_main_entity_artwork_without_a_library_or_network() {
        let html = r##"<script type="application/ld+json">{"@graph":[{"@type":"WebPage","url":"https://example.com/book","mainEntity":{"@id":"#book"}},{"@type":"Book","@id":"#book","name":"A book","image":"https://example.com/cover.jpg"},{"@type":"Book","url":"https://example.com/another-book","image":"https://example.com/other.jpg"}]}</script>"##;
        let response = serde_json::to_value(handle(request(html))).unwrap();
        assert_eq!(
            response,
            json!({"protocol_version":4,"ok":true,"preview_candidates":["https://example.com/cover.jpg"]})
        );
    }

    #[test]
    fn missing_or_unrecognized_metadata_returns_an_empty_optional_result() {
        assert_eq!(
            handle(request("")).preview_candidates,
            Some(Vec::new().into_boxed_slice())
        );
        assert_eq!(
            handle(request("<p>Book cover €20</p>")).preview_candidates,
            Some(Vec::new().into_boxed_slice())
        );
    }

    #[test]
    fn rejects_oversized_invalid_and_unsupported_requests() {
        let mut cases = vec![request(
            &"x".repeat(oia_core::product::MAX_BROWSER_PRODUCT_HTML_BYTES + 1),
        )];
        let mut version = request("");
        version.protocol_version = 3;
        cases.push(version);
        let mut action = request("");
        action.action = "save".into();
        cases.push(action);
        for value in [
            "file:///tmp/book",
            "https://user:secret@example.com/book",
            "not-a-url",
        ] {
            let mut invalid = request("");
            invalid.url = value.into();
            cases.push(invalid);
        }
        let mut long_url = request("");
        long_url.url = format!("https://example.com/{}", "x".repeat(MAX_PAGE_URL_BYTES));
        cases.push(long_url);
        for case in cases {
            let response = handle(case);
            assert!(!response.ok);
            assert_eq!(response.error.as_deref(), Some("invalid_request"));
            assert!(response.preview_candidates.is_none());
        }
    }
}
