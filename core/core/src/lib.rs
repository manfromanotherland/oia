// SPDX-License-Identifier: MIT

uniffi::setup_scaffolding!();

mod article_html;
pub mod bluesky_source;
mod card_description;
mod category_update;
mod color_search;
mod delete;
pub mod ffi;
mod frontmatter;
mod harvard_book;
pub mod highlights;
mod id;
mod images;
pub mod inbox;
pub mod index;
mod ingest;
pub mod inspector;
pub mod instagram;
pub mod link_metadata;
pub mod list;
mod locking;
mod media_dimensions;
pub mod notes;
mod page_capture;
pub mod product;
pub mod rating;
pub mod reconcile;
pub mod sanitation;
pub mod scanner;
mod search;
pub mod selection;
pub mod source_capture;
pub mod status;
pub mod tags;
mod time;
mod types;
mod url_norm;
pub mod visual_index;
mod writer;
pub mod x_source;

pub use category_update::{
    fetch_category_artwork, update_link_category_if_unchanged, CategoryUpdateOutcome,
};
pub use delete::{delete_reading, delete_unenriched_link_files_if_unchanged};
pub use frontmatter::{parse_reading, read_metadata, render_reading};
pub use highlights::{
    add_highlight, delete_highlight, list_highlights, toggle_highlight, Highlight,
};
pub use id::{media_id, new_id, quote_id, url_id};
pub use images::{first_local_image_asset, write_images, ImageBytes};
pub use inbox::{process_inbox, InboxIssue, InboxReport};
pub use index::open as open_index;
pub use ingest::{
    begin_browser_video_import, import_image, import_image_from_origin_with_options,
    import_image_with_options, import_link, import_link_capture, import_link_capture_if_unchanged,
    import_link_with_options, import_reading, import_text, import_text_with_options,
    import_video_file, import_video_file_from_origin_with_options, import_video_file_with_options,
    normalize_theme_color, save_capture, save_link_capture, save_source_capture,
    BrowserVideoImport, BrowserVideoImportInput, ExpectedArticleState, ImportOptions,
    ImportedReadingState, SaveDisposition, SaveError, SaveInput, SaveLinkInput, SaveOutcome,
    SourceCaptureInput, StagedSourceAsset, MAX_BROWSER_VIDEO_BYTES,
};
pub use link_metadata::{
    fetch_link_metadata, fetch_page_metadata, normalize_isbn, parse_link_metadata,
    DetectedLinkMetadata, DetectedPageMetadata, LinkCategory,
};
pub use list::{
    get_reading, list_readings, sidebar_counts, view_counts, CountScope, ListOptions, ReadingRow,
    SidebarCounts, SortField, View, ViewCounts,
};
pub use notes::{get_note, set_note};
pub use page_capture::{
    classify_page_capture, PageCaptureEvidence, PageCaptureKind, MAX_PAGE_CAPTURE_PARAGRAPHS,
};
pub use product::{
    fetch_product_metadata, parse_product_metadata, update_product_metadata_if_unchanged,
};
pub use rating::{list_ratings, set_rating};
pub use reconcile::{apply_diffs, rebuild};
pub use scanner::{diff, scan_library, ScanDiff, ScannedReading};
pub use selection::{
    save_selection, selection_kind, SelectionContext, SelectionKind, SelectionSaveInput,
    MAX_SELECTION_TEXT_BYTES,
};
pub use source_capture::{
    find_saved_url, save_special_url, save_url, SaveUrlError, UrlSaveRequest,
};
pub use status::{set_archived, set_favorite, set_read};
pub use tags::{add_tag, list_tags, remove_tag, MAX_TAG_LEN};
pub use types::{
    LibraryRoot, MachineTagSource, Metadata, ProductMetadata, Reading, ReadingKind,
    SourceAttachment, SourceProfile,
};
pub use url_norm::normalize_url;
pub use visual_index::{
    complete_visual_analysis, current_visual_assets, pending_visual_analysis,
    PendingVisualAnalysis, PredominantColor, VisualAnalysisResult, VisualAnalysisTask, VisualAsset,
    VisualLabel, WeightedColor,
};
pub use writer::{find_by_media, find_by_url, sha256_hex, write_reading};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
