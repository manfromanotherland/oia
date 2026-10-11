// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use oia_core::{ReadingKind, SelectionContext, SelectionKind};

/// The wire protocol version shared with the browser extension.
pub const PROTOCOL_VERSION: u32 = 4;

/// Incoming save request from the browser extension.
#[derive(Debug, Deserialize)]
pub struct SaveRequest {
    pub protocol_version: u32,
    pub action: String,
    pub metadata: RequestMetadata,
    pub markdown: String,
    /// Raw extracted-HTML measurements for automatic `save_page` classification.
    #[serde(default)]
    pub page_content: Option<oia_core::PageCaptureEvidence>,
    /// Image bytes captured by the extension. The host writes these to disk; it
    /// never downloads anything itself.
    pub images: Vec<RequestImage>,
    #[serde(default)]
    pub preview_url: Option<String>,
    #[serde(default)]
    pub favicon_url: Option<String>,
}

/// Save selected text with its whole-page capture; Rust classifies quote or highlight.
#[derive(Debug, Deserialize)]
pub struct SaveSelectionRequest {
    #[serde(flatten)]
    pub capture: SaveRequest,
    pub selected_text: String,
    #[serde(default)]
    pub selection_context: Option<SelectionContext>,
}

/// Incoming lightweight-link request with metadata/assets read from the live page.
#[derive(Debug, Deserialize)]
pub struct SaveLinkRequest {
    pub protocol_version: u32,
    pub action: String,
    pub metadata: RequestMetadata,
    pub images: Vec<RequestImage>,
    #[serde(default)]
    pub preview_url: Option<String>,
    #[serde(default)]
    pub favicon_url: Option<String>,
}

/// One image, captured and base64-encoded by the extension.
#[derive(Debug, Deserialize)]
pub struct RequestImage {
    /// Source lookup key: a Markdown URL or an explicit preview/favicon role URL.
    pub url: String,
    /// The `Content-Type` the browser saw, used to pick a file extension.
    #[serde(default)]
    pub content_type: String,
    /// Standard base64 of the raw image bytes.
    pub data_base64: String,
}

#[derive(Debug, Deserialize)]
pub struct RequestMetadata {
    #[serde(default)]
    pub kind: ReadingKind,
    pub url: String,
    #[serde(default)]
    pub media_url: Option<String>,
    pub canonical_url: String,
    pub title: String,
    pub author: Option<String>,
    pub site: Option<String>,
    #[serde(default)]
    pub theme_color: Option<String>,
    /// Bounded raw structured evidence from the live DOM; core interprets it.
    #[serde(default)]
    pub product_html: Option<String>,
    pub lang: Option<String>,
    pub excerpt: Option<String>,
    pub word_count: Option<u32>,
    pub saved_at: String,
}

/// Incoming check request — asks whether a URL is already in the library.
#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    pub protocol_version: u32,
    pub url: String,
}

/// Pure inspection of the browser's bounded structured DOM snapshot.
#[derive(Debug, Deserialize)]
pub struct InspectPageMetadataRequest {
    pub protocol_version: u32,
    pub action: String,
    pub url: String,
    pub product_html: String,
}

/// Start one connection-scoped browser video upload.
#[derive(Debug, Deserialize)]
pub struct VideoImportBeginRequest {
    pub protocol_version: u32,
    pub action: String,
    pub upload_id: String,
    pub metadata: RequestMetadata,
    pub content_type: String,
    #[serde(default)]
    pub expected_bytes: Option<u64>,
}

/// Append one base64-encoded chunk to the active browser video upload.
#[derive(Debug, Deserialize)]
pub struct VideoImportChunkRequest {
    pub protocol_version: u32,
    pub action: String,
    pub upload_id: String,
    pub sequence: u64,
    pub data_base64: String,
}

/// Finish or abort the active browser video upload.
#[derive(Debug, Deserialize)]
pub struct VideoImportEndRequest {
    pub protocol_version: u32,
    pub action: String,
    pub upload_id: String,
}

/// Outgoing response to the browser extension (shared by save and check actions).
#[derive(Debug, Serialize)]
pub struct SaveResponse {
    pub protocol_version: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Present only in responses to a `check` action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<bool>,
    /// Present only after a successful `save_selection` action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_kind: Option<SelectionKind>,
    /// Present only in a pure `inspect_page_metadata` response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_candidates: Option<Box<[String]>>,
}

impl SaveResponse {
    pub fn ack() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            ok: true,
            id: None,
            path: None,
            error: None,
            message: None,
            saved: None,
            selection_kind: None,
            preview_candidates: None,
        }
    }

    pub fn success(id: String, path: String) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            ok: true,
            id: Some(id),
            path: Some(path),
            error: None,
            message: None,
            saved: None,
            selection_kind: None,
            preview_candidates: None,
        }
    }

    pub fn error(code: &str, msg: &str) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            ok: false,
            id: None,
            path: None,
            error: Some(code.to_string()),
            message: Some(msg.to_string()),
            saved: None,
            selection_kind: None,
            preview_candidates: None,
        }
    }

    pub fn check(is_saved: bool, id: Option<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            ok: true,
            saved: Some(is_saved),
            selection_kind: None,
            preview_candidates: None,
            id,
            path: None,
            error: None,
            message: None,
        }
    }
}
