// SPDX-License-Identifier: MIT

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// The kind of content saved in a reading folder.
///
/// Older article files do not have a `kind` field, so [`Article`](Self::Article)
/// is the serde default. The lowercase wire names are shared by frontmatter and
/// the native-messaging protocol.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReadingKind {
    #[default]
    Article,
    Image,
    Video,
    Quote,
}

impl ReadingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Image => "image",
            Self::Video => "video",
            Self::Quote => "quote",
        }
    }

    pub fn is_media(self) -> bool {
        matches!(self, Self::Image | Self::Video)
    }
}

/// Durable, provider-neutral metadata for a saved source item.
///
/// Provider and kind discriminators deliberately remain strings. A newer
/// resolver can therefore write values an older core does not recognise and
/// still have them survive frontmatter parsing and indexing unchanged.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceProfile {
    pub version: u32,
    pub source_type: String,
    pub provider: String,
    pub source_id: String,
    pub author_handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_asset: Option<String>,
    /// Source order is presentation order and must be retained on round trips.
    #[serde(default)]
    pub attachments: Vec<SourceAttachment>,
}

/// One locally persisted attachment belonging to a [`SourceProfile`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceAttachment {
    /// Open discriminator such as `image` or `video`.
    pub kind: String,
    /// Reading-relative local path in `assets/`.
    pub asset: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_asset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

/// One file-backed set of inferred tags from a particular local analyzer.
///
/// The fingerprint is of the source content, rather than the reading ID, so a
/// later edit or asset replacement can be analyzed again without discarding
/// tags supplied by the other analyzer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MachineTagSource {
    pub source: String,
    pub source_fingerprint: String,
    pub analyzer_version: String,
    pub tags: Vec<String>,
}

/// All YAML frontmatter fields for a saved reading.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Metadata {
    pub format_version: u32,
    pub id: String,
    /// Missing in older files, which are always articles.
    #[serde(default)]
    pub kind: ReadingKind,
    /// A URL-only article created without captured page content. The browser
    /// extension may replace this placeholder with a full capture later while
    /// retaining the same URL-derived id and user-managed state.
    #[serde(default, skip_serializing_if = "is_false")]
    pub lightweight: bool,
    pub url: String,
    /// The clicked image/video URL. Articles and older files leave this unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_url: Option<String>,
    /// Relative path to the first locally captured preview image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_asset: Option<String>,
    /// Relative path to the locally captured page favicon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon_asset: Option<String>,
    /// Canonical sRGB page theme color (`#rrggbb`) captured from website metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>,
    pub canonical_url: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    /// Provider-neutral source metadata for special links such as social posts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_profile: Option<SourceProfile>,
    pub saved_at: String,
    /// UTC ISO-8601 timestamp (millisecond precision, `Z` suffix — same format
    /// as `saved_at`) of the most recent time the reading was marked read.
    /// `None` means unread: presence of this field *is* the read state, so there
    /// is no separate `read` boolean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<String>,
    pub archived: bool,
    pub favorite: bool,
    /// Star rating 0–5, where 0 means unrated. Defaults to 0 for articles
    /// saved before ratings existed (field absent from older frontmatter).
    #[serde(default)]
    pub rating: u8,
    pub tags: Vec<String>,
    /// Inferred tags, grouped by source so text and image updates are independent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub machine_tags: Vec<MachineTagSource>,
    /// Case-folded tombstones for inferred tags the user removed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_machine_tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    pub source_hash: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// A parsed article: its frontmatter metadata plus the Markdown body.
#[derive(Debug, Clone)]
pub struct Reading {
    pub metadata: Metadata,
    pub body: String,
}

/// A validated path to the user's library folder.
pub struct LibraryRoot(PathBuf);

impl LibraryRoot {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if !path.is_dir() {
            bail!("library root is not a directory: {}", path.display());
        }
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn articles_dir(&self) -> PathBuf {
        self.0.join("articles")
    }

    /// Unprocessed files and sealed captures, populated by external writers.
    pub fn inbox_dir(&self) -> PathBuf {
        self.0.join("inbox")
    }

    /// The self-contained folder that holds everything for one reading:
    /// `articles/<prefix>/<id>/`. The article file, its assets, and its
    /// highlights all live inside it, so a reading is one movable/deletable unit.
    pub fn reading_dir(&self, id: &str) -> PathBuf {
        self.articles_dir().join(fanout_prefix(id)).join(id)
    }

    pub fn assets_dir(&self, id: &str) -> PathBuf {
        self.reading_dir(id).join("assets")
    }

    pub fn article_path(&self, id: &str) -> PathBuf {
        self.reading_dir(id).join("article.md")
    }

    pub fn highlights_path(&self, id: &str) -> PathBuf {
        self.reading_dir(id).join("highlights.md")
    }

    /// The optional personal Markdown note attached to one reading.
    pub fn note_path(&self, id: &str) -> PathBuf {
        self.reading_dir(id).join("note.md")
    }
}

/// The fan-out sub-directory for a content-addressed id: its first two hex
/// characters. Article ids are 64-char SHA-256 hex, so this spreads reading
/// folders evenly across 256 buckets. Falls back to the whole id for ids shorter
/// than two chars so callers never panic.
fn fanout_prefix(id: &str) -> &str {
    id.get(..2).unwrap_or(id)
}
