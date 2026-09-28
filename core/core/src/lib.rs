// SPDX-License-Identifier: MIT

uniffi::setup_scaffolding!();

mod card_description;
mod color_search;
mod delete;
pub mod ffi;
mod frontmatter;
pub mod highlights;
mod id;
mod images;
pub mod inbox;
pub mod index;
mod ingest;
pub mod inspector;
pub mod instagram;
pub mod list;
mod locking;
mod media_dimensions;
pub mod notes;
pub mod rating;
pub mod reconcile;
pub mod scanner;
mod search;
pub mod source_capture;
pub mod status;
pub mod tags;
mod time;
mod types;
mod url_norm;
pub mod visual_index;
mod writer;
pub mod x_source;

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
pub use list::{
    get_reading, list_readings, sidebar_counts, view_counts, CountScope, ListOptions, ReadingRow,
    SidebarCounts, SortField, View, ViewCounts,
};
pub use notes::{get_note, set_note};
pub use rating::{list_ratings, set_rating};
pub use reconcile::{apply_diffs, rebuild};
pub use scanner::{diff, scan_library, ScanDiff, ScannedReading};
pub use source_capture::{
    find_saved_url, save_special_url, save_url, SaveUrlError, UrlSaveRequest,
};
pub use status::{set_archived, set_favorite, set_read};
pub use tags::{add_tag, list_tags, remove_tag, MAX_TAG_LEN};
pub use types::{LibraryRoot, Metadata, Reading, ReadingKind, SourceAttachment, SourceProfile};
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
