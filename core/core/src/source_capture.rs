// SPDX-License-Identifier: MIT

//! Deep URL-save orchestration for provider-aware sources.
//!
//! Callers submit a URL. This module owns provider recognition, retrieval,
//! bounded staging, and the final full-article commit. Ordinary URLs keep the
//! existing lightweight-link behavior. Provider adapters remain internal so
//! Shortcut, native messaging, and native clients cannot drift.

use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use reqwest::{
    blocking::Client,
    header::{ACCEPT, CONTENT_TYPE},
    redirect::Policy,
};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use thiserror::Error;
use url::Url;

use crate::x_source::{
    classify_x_post_url, note_tweet_refinement, parse_syndication_payload,
    parse_syndication_payload_with_note_html, syndication_url,
};
use crate::{
    find_by_url, import_link_with_options, parse_reading, save_source_capture,
    ExpectedArticleState, ImportOptions, LibraryRoot, Metadata, ReadingKind, SaveDisposition,
    SaveError, SaveInput, SaveOutcome, SourceAttachment, SourceCaptureInput, SourceProfile,
    StagedSourceAsset,
};

const METADATA_LIMIT: u64 = 2 * 1024 * 1024;
const AVATAR_LIMIT: u64 = 5 * 1024 * 1024;
const IMAGE_LIMIT: u64 = 40 * 1024 * 1024;
const VIDEO_LIMIT: u64 = 512 * 1024 * 1024;
const TOTAL_ASSET_LIMIT: u64 = 1024 * 1024 * 1024;
const MAX_ATTACHMENTS: usize = 4;
const SOURCE_FRONTMATTER_LIMIT: u64 = 256 * 1024;
const ATTACHMENT_MARKER: &str = "<!-- oia:attachments -->";
const SOURCE_RETRIEVAL_DEADLINE: Duration = Duration::from_secs(180);
const MAX_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Provider-neutral request used by every URL-only ingress path.
#[derive(Debug, Clone)]
pub struct UrlSaveRequest {
    pub url: String,
    pub title_hint: Option<String>,
    pub saved_at: Option<String>,
}

impl UrlSaveRequest {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            title_hint: None,
            saved_at: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum SaveUrlError {
    #[error("could not retrieve the source: {0}")]
    Retrieval(String),
    #[error(transparent)]
    Save(#[from] SaveError),
    #[error(transparent)]
    Storage(#[from] anyhow::Error),
}

type UrlPolicy = fn(&Url) -> bool;

#[derive(Clone, Copy)]
struct ProviderDescriptor {
    source_type: &'static str,
    provider: &'static str,
    site_name: &'static str,
    metadata_url_policy: UrlPolicy,
    asset_url_policy: UrlPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceReference {
    source_id: String,
    canonical_url: String,
}

struct ResolvedSource {
    source_id: String,
    canonical_url: String,
    text: String,
    display_name: String,
    handle: String,
    published_at: String,
    avatar_url: String,
    attachments: Vec<ResolvedSourceAttachment>,
}

struct ResolvedSourceAttachment {
    kind: ResolvedAttachmentKind,
    url: String,
    poster_url: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    alt: Option<String>,
}

#[derive(Clone, Copy)]
enum ResolvedAttachmentKind {
    Image,
    Video,
}

/// Internal seam for a recognized public source. A new provider implements
/// this interface and joins the registry; facade, identity, and persistence
/// logic remain provider-neutral.
trait SourceProvider: Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn classify(&self, url: &str) -> Option<SourceReference>;
    fn metadata_url(&self, reference: &SourceReference) -> Result<String, String>;
    fn metadata_refinement(&self, _: &[u8]) -> Result<Option<MetadataRefinement>, String> {
        Ok(None)
    }
    fn parse_metadata(
        &self,
        payload: &[u8],
        refinement: Option<&[u8]>,
    ) -> Result<ResolvedSource, String>;
}

struct MetadataRefinement {
    source_id: String,
    url: String,
    url_policy: UrlPolicy,
}

struct XSourceProvider;

impl SourceProvider for XSourceProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            source_type: "social_post",
            provider: "x",
            site_name: "X",
            metadata_url_policy: allowed_x_syndication_url,
            asset_url_policy: allowed_x_asset_url,
        }
    }

    fn classify(&self, url: &str) -> Option<SourceReference> {
        classify_x_post_url(url).map(|reference| SourceReference {
            source_id: reference.post_id,
            canonical_url: reference.canonical_url,
        })
    }

    fn metadata_url(&self, reference: &SourceReference) -> Result<String, String> {
        let reference = classify_x_post_url(&reference.canonical_url)
            .filter(|candidate| candidate.post_id == reference.source_id)
            .ok_or_else(|| "the recognized source identity was invalid".to_string())?;
        Ok(syndication_url(&reference))
    }

    fn metadata_refinement(&self, payload: &[u8]) -> Result<Option<MetadataRefinement>, String> {
        let payload = std::str::from_utf8(payload)
            .map_err(|_| "the provider returned non-UTF-8 metadata".to_string())?;
        note_tweet_refinement(payload)
            .map(|refinement| {
                refinement.map(|refinement| MetadataRefinement {
                    source_id: refinement.source_id,
                    url: refinement.canonical_url,
                    url_policy: allowed_x_post_url,
                })
            })
            .map_err(|error| error.to_string())
    }

    fn parse_metadata(
        &self,
        payload: &[u8],
        refinement: Option<&[u8]>,
    ) -> Result<ResolvedSource, String> {
        let payload = std::str::from_utf8(payload)
            .map_err(|_| "the provider returned non-UTF-8 metadata".to_string())?;
        let post = match refinement {
            Some(html) => {
                let html = std::str::from_utf8(html)
                    .map_err(|_| "the provider returned non-UTF-8 metadata".to_string())?;
                parse_syndication_payload_with_note_html(payload, html)
            }
            None => parse_syndication_payload(payload),
        }
        .map_err(|error| error.to_string())?;
        Ok(ResolvedSource {
            source_id: post.source_id,
            canonical_url: post.canonical_url,
            text: post.text,
            display_name: post.display_name,
            handle: post.handle,
            published_at: post.published_at,
            avatar_url: post.avatar_url,
            attachments: post
                .attachments
                .into_iter()
                .map(|attachment| ResolvedSourceAttachment {
                    kind: match attachment.kind {
                        crate::x_source::AttachmentKind::Image => ResolvedAttachmentKind::Image,
                        crate::x_source::AttachmentKind::Video => ResolvedAttachmentKind::Video,
                    },
                    url: attachment.url,
                    poster_url: attachment.poster_url,
                    width: attachment.width,
                    height: attachment.height,
                    alt: attachment.alt,
                })
                .collect(),
        })
    }
}

static X_PROVIDER: XSourceProvider = XSourceProvider;
static SOURCE_PROVIDERS: [&'static dyn SourceProvider; 1] = [&X_PROVIDER];

struct RecognizedSource<'a> {
    provider: &'a dyn SourceProvider,
    reference: SourceReference,
}

fn recognize_source<'a>(
    providers: &'a [&dyn SourceProvider],
    url: &str,
) -> Option<RecognizedSource<'a>> {
    providers.iter().find_map(|provider| {
        provider.classify(url).map(|reference| RecognizedSource {
            provider: *provider,
            reference,
        })
    })
}

/// A saved t.co link can carry the resolved post URL from its original browser
/// capture. Use only that file-backed evidence: a new short URL or another
/// site's canonical hint never authorizes a social-post replacement.
fn recognize_saved_source<'a>(
    library: &LibraryRoot,
    providers: &'a [&dyn SourceProvider],
    submitted_url: &str,
) -> Option<RecognizedSource<'a>> {
    if let Some(source) = recognize_source(providers, submitted_url) {
        return Some(source);
    }
    if !valid_x_shortlink_url(submitted_url) {
        return None;
    }

    let id = crate::url_id(submitted_url).ok()?;
    let article_path = library.article_path(&id);
    let metadata = read_source_metadata(&article_path).ok()?;
    if !valid_article_identity(library, article_path.parent()?, &metadata) {
        return None;
    }
    let canonical = saved_x_shortlink_canonical(&metadata)?;
    recognize_source(providers, canonical)
}

fn valid_x_shortlink_url(raw_url: &str) -> bool {
    let Ok(url) = Url::parse(raw_url.trim()) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str() != Some("t.co")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    let Some(mut segments) = url.path_segments() else {
        return false;
    };
    let Some(token) = segments.next() else {
        return false;
    };
    (1..=64).contains(&token.len())
        && token.bytes().all(|byte| byte.is_ascii_alphanumeric())
        && segments.next().is_none()
}

fn saved_x_shortlink_canonical(metadata: &Metadata) -> Option<&str> {
    if !valid_x_shortlink_url(&metadata.url) {
        return None;
    }
    let reference = classify_x_post_url(&metadata.canonical_url)?;
    let supported_state = match metadata.source_profile.as_ref() {
        None => metadata.lightweight,
        Some(profile) => {
            !metadata.lightweight
                && valid_matching_profile(profile, X_PROVIDER.descriptor(), &reference.post_id)
        }
    };
    supported_state.then_some(metadata.canonical_url.as_str())
}

/// Save a URL through the provider registry, falling back to the ordinary
/// lightweight link representation when no provider recognises it.
pub fn save_url(
    library: &LibraryRoot,
    request: UrlSaveRequest,
) -> Result<SaveOutcome, SaveUrlError> {
    if let Some(outcome) = save_special_url(library, &request)? {
        return Ok(outcome);
    }

    save_ordinary_url_with(
        library,
        request,
        crate::fetch_page_metadata,
        crate::fetch_category_artwork,
    )
}

fn save_ordinary_url_with(
    library: &LibraryRoot,
    request: UrlSaveRequest,
    fetch: impl FnOnce(&str) -> anyhow::Result<crate::DetectedPageMetadata>,
    fetch_artwork: impl FnOnce(&str) -> anyhow::Result<crate::ImageBytes>,
) -> Result<SaveOutcome, SaveUrlError> {
    if let Some(id) = find_by_url(library, &request.url)? {
        if crate::scanner::read_reading_for_id(library, &id)?.is_some_and(|reading| {
            reading
                .metadata
                .link_category
                .as_deref()
                .and_then(crate::LinkCategory::parse)
                .is_some()
        }) {
            return Ok(duplicate_outcome(library, id));
        }
    }
    // One optional HTML retrieval feeds both classifiers. Narrow provider
    // routes retain their category even when HTTP is unavailable.
    let detected = fetch(&request.url).unwrap_or_else(|_| crate::DetectedPageMetadata {
        link: crate::parse_link_metadata("", &request.url),
        product: None,
    });
    if detected.link.is_some() || detected.product.is_some() {
        let artwork = detected
            .link
            .as_ref()
            .and_then(|link| link.image_url.as_deref())
            .and_then(|url| fetch_artwork(url).ok());
        let preview_url = artwork.as_ref().map(|image| image.url.clone());
        return Ok(crate::save_link_capture(
            library,
            crate::SaveLinkInput {
                url: request.url.clone(),
                canonical_url: request.url.clone(),
                title: detected
                    .link
                    .as_ref()
                    .and_then(|link| link.title.clone())
                    .or(request.title_hint)
                    .unwrap_or_else(|| request.url.clone()),
                author: None,
                site: None,
                saved_at: request.saved_at.unwrap_or_else(crate::time::now_utc_iso),
                images: artwork.into_iter().collect(),
                preview_url,
                favicon_url: None,
                theme_color: None,
                product: detected.product,
                link_category: detected.link.map(|link| link.category),
                excerpt: None,
                lang: None,
            },
        )?);
    }
    Ok(import_link_with_options(
        library,
        &request.url,
        ImportOptions {
            title: request.title_hint,
            saved_at: request.saved_at,
            ..ImportOptions::default()
        },
    )?)
}

/// Resolve and save a recognised provider URL. `None` means the URL is an
/// ordinary link; a positively recognised provider never silently degrades.
pub fn save_special_url(
    library: &LibraryRoot,
    request: &UrlSaveRequest,
) -> Result<Option<SaveOutcome>, SaveUrlError> {
    save_special_url_with(library, request, &HttpRetriever)
}

/// Check whether a submitted URL is already represented in the file-backed
/// library, including provider aliases that share one stable source id.
pub fn find_saved_url(
    library: &LibraryRoot,
    submitted_url: &str,
) -> Result<Option<String>, SaveUrlError> {
    find_saved_url_with_providers(library, submitted_url, &SOURCE_PROVIDERS)
}

fn find_saved_url_with_providers(
    library: &LibraryRoot,
    submitted_url: &str,
    providers: &[&dyn SourceProvider],
) -> Result<Option<String>, SaveUrlError> {
    if let Some(source) = recognize_saved_source(library, providers, submitted_url) {
        if let Some(id) = find_by_url(library, submitted_url)? {
            return Ok(Some(id));
        }
        if let Some(id) = find_by_url(library, &source.reference.canonical_url)? {
            return Ok(Some(id));
        }
        let matches = scan_source_matches(library, source.provider, &source.reference)?;
        if let Some(existing) = matches.source {
            return Ok(Some(existing.metadata.id));
        }
        if let Some(existing) = matches.legacy {
            return Ok(Some(existing.metadata.id));
        }
        return Ok(None);
    }

    find_by_url(library, submitted_url).map_err(Into::into)
}

fn save_special_url_with(
    library: &LibraryRoot,
    request: &UrlSaveRequest,
    retriever: &impl SourceRetriever,
) -> Result<Option<SaveOutcome>, SaveUrlError> {
    save_special_url_with_timeout(library, request, retriever, SOURCE_RETRIEVAL_DEADLINE)
}

fn save_special_url_with_timeout(
    library: &LibraryRoot,
    request: &UrlSaveRequest,
    retriever: &impl SourceRetriever,
    retrieval_timeout: Duration,
) -> Result<Option<SaveOutcome>, SaveUrlError> {
    save_special_url_with_providers(
        library,
        request,
        retriever,
        &SOURCE_PROVIDERS,
        retrieval_timeout,
    )
}

fn save_special_url_with_providers(
    library: &LibraryRoot,
    request: &UrlSaveRequest,
    retriever: &impl SourceRetriever,
    providers: &[&dyn SourceProvider],
    retrieval_timeout: Duration,
) -> Result<Option<SaveOutcome>, SaveUrlError> {
    let Some(source) = recognize_saved_source(library, providers, &request.url) else {
        return Ok(None);
    };

    let matches = scan_source_matches(library, source.provider, &source.reference)?;
    let existing_source = matches.source.and_then(|candidate| {
        existing_source_from_candidate(library, source.provider, &source.reference, candidate)
    });

    if let Some(existing) = existing_source.as_ref() {
        if source_assets_complete(library, existing) {
            return Ok(Some(duplicate_outcome(library, existing.id.clone())));
        }
    }

    let identity = if let Some(existing) = existing_source {
        IdentitySelection::SaveAsExisting {
            url: existing.url,
            expected_article: ExpectedArticleState::Sha256(existing.article_sha256),
        }
    } else {
        select_identity(
            library,
            &request.url,
            source.provider,
            &source.reference,
            matches.legacy,
        )?
    };
    if let IdentitySelection::Duplicate(id) = identity {
        return Ok(Some(duplicate_outcome(library, id)));
    }

    let deadline = RetrievalDeadline::new(retrieval_timeout);
    let request_timeout = deadline
        .request_timeout()
        .map_err(SaveUrlError::Retrieval)?;
    let metadata_url = source
        .provider
        .metadata_url(&source.reference)
        .map_err(SaveUrlError::Retrieval)?;
    let descriptor = source.provider.descriptor();
    let payload = retriever
        .fetch_metadata(
            &metadata_url,
            descriptor.metadata_url_policy,
            METADATA_LIMIT,
            request_timeout,
        )
        .map_err(SaveUrlError::Retrieval)?;
    deadline
        .ensure_remaining()
        .map_err(SaveUrlError::Retrieval)?;
    let refinement = source
        .provider
        .metadata_refinement(&payload)
        .map_err(SaveUrlError::Retrieval)?;
    let refinement_payload = refinement
        .map(|refinement| {
            if refinement.source_id != source.reference.source_id {
                return Err(SaveUrlError::Retrieval(
                    "the provider returned a different post identifier".into(),
                ));
            }
            let timeout = deadline
                .request_timeout()
                .map_err(SaveUrlError::Retrieval)?;
            let bytes = retriever
                .fetch_metadata(
                    &refinement.url,
                    refinement.url_policy,
                    METADATA_LIMIT,
                    timeout,
                )
                .map_err(SaveUrlError::Retrieval)?;
            deadline
                .ensure_remaining()
                .map_err(SaveUrlError::Retrieval)?;
            Ok(bytes)
        })
        .transpose()?;
    let post = source
        .provider
        .parse_metadata(&payload, refinement_payload.as_deref())
        .map_err(SaveUrlError::Retrieval)?;
    if post.source_id != source.reference.source_id {
        return Err(SaveUrlError::Retrieval(
            "the provider returned a different post identifier".into(),
        ));
    }
    if post.attachments.len() > MAX_ATTACHMENTS {
        return Err(SaveUrlError::Retrieval(format!(
            "the post has more than {MAX_ATTACHMENTS} supported attachments"
        )));
    }

    let canonical_url = post.canonical_url.clone();
    let (identity_url, expected_article) = match identity {
        IdentitySelection::SaveAsExisting {
            url,
            expected_article,
        } => (url, expected_article),
        IdentitySelection::New => (canonical_url.clone(), ExpectedArticleState::Missing),
        IdentitySelection::Duplicate(_) => unreachable!("duplicate identity returned above"),
    };

    let staging = TempDir::new().map_err(anyhow::Error::from)?;
    let prepared = prepare_post(
        retriever,
        &staging,
        post,
        descriptor.asset_url_policy,
        &deadline,
    )?;
    let saved_at = request
        .saved_at
        .clone()
        .unwrap_or_else(crate::time::now_utc_iso);
    let markdown = portable_markdown(&prepared.text, &prepared.attachments);
    let capture = SaveInput {
        quote_identity_markdown: None,
        kind: ReadingKind::Article,
        lightweight: false,
        url: identity_url,
        media_url: None,
        canonical_url,
        title: format!("Post by {}", prepared.display_name),
        author: Some(prepared.display_name),
        site: Some(descriptor.site_name.into()),
        saved_at,
        markdown,
        images: vec![],
        preview_url: None,
        favicon_url: None,
        theme_color: None,
        product: None,
        link_category: None,
        excerpt: Some(prepared.text.clone()),
        word_count: Some(prepared.text.split_whitespace().count() as u32),
        lang: None,
    };
    let profile = SourceProfile {
        version: 1,
        source_type: descriptor.source_type.into(),
        provider: descriptor.provider.into(),
        source_id: source.reference.source_id,
        author_handle: prepared.handle,
        published_at: Some(prepared.published_at),
        avatar_asset: prepared.avatar_asset,
        attachments: prepared.attachments,
    };

    save_source_capture(
        library,
        SourceCaptureInput {
            capture,
            source_profile: profile,
            staged_assets: prepared.staged_assets,
            expected_article,
        },
    )
    .map(Some)
    .map_err(Into::into)
}

enum IdentitySelection {
    SaveAsExisting {
        url: String,
        expected_article: ExpectedArticleState,
    },
    New,
    Duplicate(String),
}

fn select_identity(
    library: &LibraryRoot,
    submitted_url: &str,
    provider: &dyn SourceProvider,
    reference: &SourceReference,
    legacy: Option<ArticleCandidate>,
) -> Result<IdentitySelection, SaveUrlError> {
    let submitted = crate::normalize_url(submitted_url)
        .map_err(|error| SaveError::InvalidRequest(error.to_string()))?;
    if let Some(selection) = identity_for_existing_url(library, &submitted, provider, reference)? {
        return Ok(selection);
    }
    if submitted != reference.canonical_url {
        if let Some(selection) =
            identity_for_existing_url(library, &reference.canonical_url, provider, reference)?
        {
            return Ok(selection);
        }
    }
    if let Some(existing) = legacy.and_then(|candidate| {
        existing_legacy_from_candidate(library, provider, reference, candidate)
    }) {
        return Ok(IdentitySelection::SaveAsExisting {
            url: existing.url,
            expected_article: ExpectedArticleState::Sha256(existing.article_sha256),
        });
    }
    Ok(IdentitySelection::New)
}

fn identity_for_existing_url(
    library: &LibraryRoot,
    url: &str,
    provider: &dyn SourceProvider,
    reference: &SourceReference,
) -> Result<Option<IdentitySelection>, SaveUrlError> {
    let Some(id) = find_by_url(library, url)? else {
        return Ok(None);
    };
    let Ok(snapshot) = article_snapshot(&library.article_path(&id)) else {
        return Ok(Some(IdentitySelection::Duplicate(id)));
    };
    let metadata = snapshot.metadata;
    if !valid_source_reading_id(&metadata.id)
        || metadata.id != id
        || metadata.kind != ReadingKind::Article
        || !crate::url_id(&metadata.url).is_ok_and(|stored_id| stored_id == id)
        || !origin_matches(provider, reference, &metadata)
    {
        return Ok(Some(IdentitySelection::Duplicate(id)));
    }
    let replaceable = metadata.source_profile.as_ref().is_none_or(|profile| {
        profile_matches_identity(profile, provider.descriptor(), &reference.source_id)
    });
    Ok(Some(if replaceable {
        IdentitySelection::SaveAsExisting {
            url: metadata.url,
            expected_article: ExpectedArticleState::Sha256(snapshot.article_sha256),
        }
    } else {
        IdentitySelection::Duplicate(id)
    }))
}

struct ExistingSource {
    id: String,
    url: String,
    profile: SourceProfile,
    article_sha256: String,
}

struct ExistingArticleIdentity {
    url: String,
    article_sha256: String,
}

struct ArticleCandidate {
    article_path: PathBuf,
    metadata: Metadata,
}

#[derive(Default)]
struct SourceMatches {
    source: Option<ArticleCandidate>,
    legacy: Option<ArticleCandidate>,
}

struct ArticleSnapshot {
    metadata: Metadata,
    article_sha256: String,
}

fn scan_source_matches(
    library: &LibraryRoot,
    provider: &dyn SourceProvider,
    reference: &SourceReference,
) -> Result<SourceMatches, SaveUrlError> {
    let mut matches = SourceMatches::default();
    let articles = library.articles_dir();
    let prefixes = match fs::read_dir(&articles) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(matches),
        Err(error) => return Err(anyhow::Error::from(error).into()),
    };
    for prefix in prefixes {
        let Ok(prefix) = prefix else { continue };
        if !prefix.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(readings) = fs::read_dir(prefix.path()) else {
            continue;
        };
        for reading in readings.flatten() {
            if !reading.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let article = reading.path().join("article.md");
            let Ok(metadata) = read_source_metadata(&article) else {
                continue;
            };
            if !valid_article_identity(library, &reading.path(), &metadata)
                || !origin_matches(provider, reference, &metadata)
            {
                continue;
            }

            let is_source = !metadata.lightweight
                && metadata.source_profile.as_ref().is_some_and(|profile| {
                    valid_matching_profile(profile, provider.descriptor(), &reference.source_id)
                });
            let is_legacy = metadata.source_profile.is_none();
            let candidate = ArticleCandidate {
                article_path: article,
                metadata,
            };
            if is_source {
                retain_earliest(&mut matches.source, candidate);
            } else if is_legacy {
                retain_earliest(&mut matches.legacy, candidate);
            }
        }
    }
    Ok(matches)
}

fn retain_earliest(slot: &mut Option<ArticleCandidate>, candidate: ArticleCandidate) {
    let replace = slot.as_ref().is_none_or(|current| {
        (&candidate.metadata.saved_at, &candidate.metadata.id)
            < (&current.metadata.saved_at, &current.metadata.id)
    });
    if replace {
        *slot = Some(candidate);
    }
}

fn existing_source_from_candidate(
    library: &LibraryRoot,
    provider: &dyn SourceProvider,
    reference: &SourceReference,
    candidate: ArticleCandidate,
) -> Option<ExistingSource> {
    let snapshot = article_snapshot(&candidate.article_path).ok()?;
    let metadata = snapshot.metadata;
    if metadata.lightweight
        || !valid_article_identity(library, candidate.article_path.parent()?, &metadata)
        || !origin_matches(provider, reference, &metadata)
    {
        return None;
    }
    let profile = metadata.source_profile.filter(|profile| {
        valid_matching_profile(profile, provider.descriptor(), &reference.source_id)
    })?;
    Some(ExistingSource {
        id: metadata.id,
        url: metadata.url,
        profile,
        article_sha256: snapshot.article_sha256,
    })
}

fn existing_legacy_from_candidate(
    library: &LibraryRoot,
    provider: &dyn SourceProvider,
    reference: &SourceReference,
    candidate: ArticleCandidate,
) -> Option<ExistingArticleIdentity> {
    let snapshot = article_snapshot(&candidate.article_path).ok()?;
    let metadata = snapshot.metadata;
    if metadata.source_profile.is_some()
        || !valid_article_identity(library, candidate.article_path.parent()?, &metadata)
        || !origin_matches(provider, reference, &metadata)
    {
        return None;
    }
    Some(ExistingArticleIdentity {
        url: metadata.url,
        article_sha256: snapshot.article_sha256,
    })
}

fn origin_matches(
    provider: &dyn SourceProvider,
    reference: &SourceReference,
    metadata: &Metadata,
) -> bool {
    provider
        .classify(&metadata.url)
        .is_some_and(|candidate| candidate.source_id == reference.source_id)
        || (provider.descriptor().provider == "x"
            && saved_x_shortlink_canonical(metadata)
                .and_then(|canonical| provider.classify(canonical))
                .is_some_and(|candidate| candidate.source_id == reference.source_id))
}

fn profile_matches_identity(
    profile: &SourceProfile,
    descriptor: ProviderDescriptor,
    source_id: &str,
) -> bool {
    profile
        .source_type
        .eq_ignore_ascii_case(descriptor.source_type)
        && profile.provider.eq_ignore_ascii_case(descriptor.provider)
        && profile.source_id == source_id
}

fn valid_matching_profile(
    profile: &SourceProfile,
    descriptor: ProviderDescriptor,
    source_id: &str,
) -> bool {
    profile.version > 0
        && profile_matches_identity(profile, descriptor, source_id)
        && !profile.author_handle.trim().is_empty()
        && profile.attachments.iter().all(|attachment| {
            !attachment.kind.trim().is_empty()
                && attachment.width != Some(0)
                && attachment.height != Some(0)
        })
}

fn read_source_metadata(path: &Path) -> anyhow::Result<Metadata> {
    let mut file = File::open(path)?;
    read_source_metadata_from(&mut file)
}

fn read_source_metadata_from(reader: &mut impl Read) -> anyhow::Result<Metadata> {
    let mut reader = BufReader::new(reader.take(SOURCE_FRONTMATTER_LIMIT + 1));
    let mut header = Vec::new();
    let mut line = Vec::new();
    let mut fences = 0;

    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if header.len() as u64 + line.len() as u64 > SOURCE_FRONTMATTER_LIMIT {
            return Err(anyhow::anyhow!(
                "source frontmatter exceeds the {SOURCE_FRONTMATTER_LIMIT}-byte limit"
            ));
        }
        if std::str::from_utf8(&line).is_ok_and(|line| line.trim_end() == "---") {
            fences += 1;
        }
        header.extend_from_slice(&line);
        if fences == 2 {
            break;
        }
    }

    let header = std::str::from_utf8(&header)
        .map_err(|_| anyhow::anyhow!("source frontmatter was not UTF-8"))?;
    parse_reading(header).map(|reading| reading.metadata)
}

/// Re-open only the candidate selected by the bounded metadata scan and stream
/// its complete hash. The hash is the compare-before-swap guard used after
/// network retrieval; the article body is never retained in memory.
fn article_snapshot(path: &Path) -> anyhow::Result<ArticleSnapshot> {
    let mut file = File::open(path)?;
    let metadata = read_source_metadata_from(&mut file)?;
    file.seek(std::io::SeekFrom::Start(0))?;

    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(ArticleSnapshot {
        metadata,
        article_sha256: hex::encode(hasher.finalize()),
    })
}

fn valid_article_identity(
    library: &LibraryRoot,
    reading_path: &Path,
    metadata: &crate::Metadata,
) -> bool {
    valid_source_reading_id(&metadata.id)
        && metadata.kind == ReadingKind::Article
        && reading_path == library.reading_dir(&metadata.id)
        && crate::url_id(&metadata.url).is_ok_and(|id| id == metadata.id)
}

fn valid_source_reading_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn source_assets_complete(library: &LibraryRoot, existing: &ExistingSource) -> bool {
    if existing.profile.attachments.len() > MAX_ATTACHMENTS {
        return false;
    }

    let mut requirements = Vec::new();
    if let Some(avatar) = existing.profile.avatar_asset.as_deref() {
        requirements.push((avatar, AVATAR_LIMIT));
    }
    for attachment in &existing.profile.attachments {
        let media_limit = if attachment.kind.eq_ignore_ascii_case("image") {
            IMAGE_LIMIT
        } else if attachment.kind.eq_ignore_ascii_case("video") {
            VIDEO_LIMIT
        } else {
            // Future open-string attachment roles remain readable, but never
            // escape the aggregate source-capture budget.
            TOTAL_ASSET_LIMIT
        };
        requirements.push((attachment.asset.as_str(), media_limit));
        if let Some(poster) = attachment.poster_asset.as_deref() {
            requirements.push((poster, IMAGE_LIMIT));
        }
    }

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    let mut total_bytes = 0_u64;
    for (asset, role_limit) in requirements {
        if !seen.insert(asset) {
            continue;
        }
        let Some(candidate) =
            content_addressed_asset_candidate(library, &existing.id, asset, role_limit)
        else {
            return false;
        };
        let Some(total) = total_bytes.checked_add(candidate.byte_count) else {
            return false;
        };
        if total > TOTAL_ASSET_LIMIT {
            return false;
        }
        total_bytes = total;
        candidates.push(candidate);
    }

    candidates.into_iter().all(|candidate| {
        hash_path_bounded(&candidate.path, candidate.byte_count)
            .is_some_and(|hash| hash == candidate.expected_hash)
    })
}

struct AssetCandidate {
    path: PathBuf,
    expected_hash: String,
    byte_count: u64,
}

fn content_addressed_asset_candidate(
    library: &LibraryRoot,
    id: &str,
    asset: &str,
    role_limit: u64,
) -> Option<AssetCandidate> {
    let filename = asset
        .strip_prefix("assets/")
        .filter(|filename| !filename.is_empty() && !filename.contains(['/', '\\', '\0']))?;
    let (expected_hash, extension) = filename.rsplit_once('.')?;
    if expected_hash.len() != 64
        || !expected_hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || extension.is_empty()
        || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }
    let path = library.reading_dir(id).join(asset);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return None;
    };
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > role_limit {
        return None;
    }
    Some(AssetCandidate {
        path,
        expected_hash: expected_hash.to_string(),
        byte_count: metadata.len(),
    })
}

fn hash_path_bounded(path: &Path, expected_size: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    if file.metadata().ok()?.len() != expected_size {
        return None;
    }
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            return (total == expected_size).then(|| hex::encode(hasher.finalize()));
        }
        total = total.checked_add(count as u64)?;
        if total > expected_size {
            return None;
        }
        hasher.update(&buffer[..count]);
    }
}

fn duplicate_outcome(library: &LibraryRoot, id: String) -> SaveOutcome {
    let path = library
        .article_path(&id)
        .strip_prefix(library.path())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| {
            let prefix = id.get(..2).unwrap_or(id.as_str());
            format!("articles/{prefix}/{id}/article.md")
        });
    SaveOutcome {
        disposition: SaveDisposition::Duplicate,
        id,
        path,
    }
}

struct PreparedPost {
    text: String,
    display_name: String,
    handle: String,
    published_at: String,
    avatar_asset: Option<String>,
    attachments: Vec<SourceAttachment>,
    staged_assets: Vec<StagedSourceAsset>,
}

fn prepare_post(
    retriever: &impl SourceRetriever,
    staging: &TempDir,
    post: ResolvedSource,
    asset_url_policy: UrlPolicy,
    deadline: &RetrievalDeadline,
) -> Result<PreparedPost, SaveUrlError> {
    let mut staged_assets = Vec::new();
    let mut staged_paths = HashSet::new();
    let mut total_bytes = 0_u64;

    let avatar_asset = match stage_download(
        retriever,
        staging.path(),
        &post.avatar_url,
        AssetRole::Image,
        asset_url_policy,
        AVATAR_LIMIT,
        deadline,
    ) {
        Ok(download) => {
            total_bytes = checked_total(total_bytes, download.byte_count)?;
            let asset = download.asset.clone();
            insert_staged(&mut staged_assets, &mut staged_paths, download)?;
            Some(asset)
        }
        // An avatar is decoration; losing it must not discard the saved post.
        Err(_) => None,
    };
    deadline
        .ensure_remaining()
        .map_err(SaveUrlError::Retrieval)?;

    let mut attachments = Vec::with_capacity(post.attachments.len());
    for attachment in post.attachments {
        let role = match attachment.kind {
            ResolvedAttachmentKind::Image => AssetRole::Image,
            ResolvedAttachmentKind::Video => AssetRole::Video,
        };
        let limit = match role {
            AssetRole::Image => IMAGE_LIMIT,
            AssetRole::Video => VIDEO_LIMIT,
        };
        let media = stage_download(
            retriever,
            staging.path(),
            &attachment.url,
            role,
            asset_url_policy,
            limit,
            deadline,
        )
        .map_err(SaveUrlError::Retrieval)?;
        total_bytes = checked_total(total_bytes, media.byte_count)?;
        let asset = media.asset.clone();
        let content_type = Some(media.content_type.clone());
        insert_staged(&mut staged_assets, &mut staged_paths, media)?;

        let poster_asset = attachment
            .poster_url
            .as_deref()
            .map(|url| {
                let poster = stage_download(
                    retriever,
                    staging.path(),
                    url,
                    AssetRole::Image,
                    asset_url_policy,
                    IMAGE_LIMIT,
                    deadline,
                )
                .map_err(SaveUrlError::Retrieval)?;
                total_bytes = checked_total(total_bytes, poster.byte_count)?;
                let asset = poster.asset.clone();
                insert_staged(&mut staged_assets, &mut staged_paths, poster)?;
                Ok::<_, SaveUrlError>(asset)
            })
            .transpose()?;

        attachments.push(SourceAttachment {
            kind: match attachment.kind {
                ResolvedAttachmentKind::Image => "image",
                ResolvedAttachmentKind::Video => "video",
            }
            .into(),
            asset,
            poster_asset,
            content_type,
            width: attachment.width,
            height: attachment.height,
            alt: attachment.alt,
        });
    }

    Ok(PreparedPost {
        text: post.text,
        display_name: post.display_name,
        handle: post.handle,
        published_at: post.published_at,
        avatar_asset,
        attachments,
        staged_assets,
    })
}

fn checked_total(current: u64, added: u64) -> Result<u64, SaveUrlError> {
    let total = current.checked_add(added).ok_or_else(|| {
        SaveUrlError::Retrieval("the post media size overflowed its supported limit".into())
    })?;
    if total > TOTAL_ASSET_LIMIT {
        return Err(SaveUrlError::Retrieval(format!(
            "the post media exceeds the {} MiB capture limit",
            TOTAL_ASSET_LIMIT / 1024 / 1024
        )));
    }
    Ok(total)
}

fn insert_staged(
    staged_assets: &mut Vec<StagedSourceAsset>,
    staged_paths: &mut HashSet<String>,
    download: DownloadedAsset,
) -> Result<(), SaveUrlError> {
    if staged_paths.insert(download.asset.clone()) {
        staged_assets.push(StagedSourceAsset::new(
            download.path,
            download.asset,
            download.sha256,
            download.byte_count,
        )?);
    } else {
        let _ = fs::remove_file(download.path);
    }
    Ok(())
}

fn portable_markdown(text: &str, attachments: &[SourceAttachment]) -> String {
    let mut markdown = markdown_text(text);
    if attachments.is_empty() {
        return markdown;
    }
    markdown.push_str("\n\n");
    markdown.push_str(ATTACHMENT_MARKER);
    for attachment in attachments {
        let alt = markdown_alt(attachment.alt.as_deref().unwrap_or(
            match attachment.kind.as_str() {
                "video" => "Video preview",
                _ => "Attached image",
            },
        ));
        if attachment.kind == "video" {
            if let Some(poster) = attachment.poster_asset.as_deref() {
                markdown.push_str(&format!("\n\n![{alt}]({poster})"));
            }
            markdown.push_str(&format!("\n\n[Play local video]({})", attachment.asset));
        } else {
            markdown.push_str(&format!("\n\n![{alt}]({})", attachment.asset));
        }
    }
    markdown
}

fn markdown_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    let mut at_line_start = true;
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                escaped.push('\n');
                at_line_start = true;
            }
            '\n' => {
                escaped.push('\n');
                at_line_start = true;
            }
            ' ' if at_line_start => escaped.push_str("&#32;"),
            '\t' if at_line_start => escaped.push_str("&#9;"),
            _ => {
                at_line_start = false;
                if character.is_ascii_punctuation() {
                    escaped.push('\\');
                }
                escaped.push(character);
            }
        }
    }
    escaped
}

fn markdown_alt(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace(['\r', '\n'], " ")
}

#[derive(Clone, Copy)]
enum AssetRole {
    Image,
    Video,
}

struct DownloadedAsset {
    path: PathBuf,
    asset: String,
    sha256: String,
    byte_count: u64,
    content_type: String,
}

struct DownloadReceipt {
    sha256: String,
    byte_count: u64,
    content_type: String,
}

struct RetrievalDeadline {
    expires_at: Instant,
}

impl RetrievalDeadline {
    fn new(timeout: Duration) -> Self {
        Self {
            expires_at: Instant::now() + timeout,
        }
    }

    fn request_timeout(&self) -> Result<Duration, String> {
        self.remaining()
            .map(|remaining| remaining.min(MAX_REQUEST_TIMEOUT))
    }

    fn ensure_remaining(&self) -> Result<(), String> {
        self.remaining().map(|_| ())
    }

    fn remaining(&self) -> Result<Duration, String> {
        self.expires_at
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| "the source retrieval deadline was exceeded".to_string())
    }
}

trait SourceRetriever {
    fn fetch_metadata(
        &self,
        url: &str,
        allowed_url: UrlPolicy,
        limit: u64,
        timeout: Duration,
    ) -> Result<Vec<u8>, String>;
    fn download(
        &self,
        url: &str,
        destination: &Path,
        role: AssetRole,
        allowed_url: UrlPolicy,
        limit: u64,
        timeout: Duration,
    ) -> Result<DownloadReceipt, String>;
}

fn stage_download(
    retriever: &impl SourceRetriever,
    staging: &Path,
    url: &str,
    role: AssetRole,
    allowed_url: UrlPolicy,
    limit: u64,
    deadline: &RetrievalDeadline,
) -> Result<DownloadedAsset, String> {
    let path = staging.join(format!("download-{}", crate::new_id()));
    let timeout = deadline.request_timeout()?;
    let receipt = retriever.download(url, &path, role, allowed_url, limit, timeout)?;
    deadline.ensure_remaining()?;
    let extension = extension_for_content_type(&receipt.content_type).ok_or_else(|| {
        format!(
            "the provider returned unsupported media type {}",
            receipt.content_type
        )
    })?;
    Ok(DownloadedAsset {
        path,
        asset: format!("assets/{}.{}", receipt.sha256, extension),
        sha256: receipt.sha256,
        byte_count: receipt.byte_count,
        content_type: receipt.content_type,
    })
}

struct HttpRetriever;

impl SourceRetriever for HttpRetriever {
    fn fetch_metadata(
        &self,
        url: &str,
        allowed_url: UrlPolicy,
        limit: u64,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        let parsed = Url::parse(url).map_err(|error| error.to_string())?;
        if !allowed_url(&parsed) {
            return Err("the provider metadata URL was not allowed".into());
        }
        let client = http_client(allowed_url, timeout)?;
        let mut response = client
            .get(parsed)
            .header(ACCEPT, "application/json")
            .send()
            .map_err(|error| error.to_string())?;
        validate_response(&response, allowed_url, limit)?;
        let mut bytes = Vec::new();
        read_bounded(&mut response, &mut bytes, limit)?;
        Ok(bytes)
    }

    fn download(
        &self,
        url: &str,
        destination: &Path,
        role: AssetRole,
        allowed_url: UrlPolicy,
        limit: u64,
        timeout: Duration,
    ) -> Result<DownloadReceipt, String> {
        let parsed = Url::parse(url).map_err(|error| error.to_string())?;
        if !allowed_url(&parsed) {
            return Err("the provider media URL was not allowed".into());
        }
        let client = http_client(allowed_url, timeout)?;
        let mut response = client
            .get(parsed)
            .send()
            .map_err(|error| error.to_string())?;
        validate_response(&response, allowed_url, limit)?;
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| "the provider media response had no content type".to_string())?;
        if !role.accepts(&content_type) {
            return Err(format!(
                "the provider returned {content_type} for an unsupported media role"
            ));
        }

        let write_result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|error| error.to_string())?;
            let mut hasher = Sha256::new();
            let mut byte_count = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let count = response
                    .read(&mut buffer)
                    .map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                byte_count = byte_count
                    .checked_add(count as u64)
                    .ok_or_else(|| "the provider media size overflowed".to_string())?;
                if byte_count > limit {
                    return Err(format!("the provider media exceeds the {limit}-byte limit"));
                }
                hasher.update(&buffer[..count]);
                file.write_all(&buffer[..count])
                    .map_err(|error| error.to_string())?;
            }
            if byte_count == 0 {
                return Err("the provider returned an empty media file".into());
            }
            file.sync_all().map_err(|error| error.to_string())?;
            drop(file);
            let mut validation_file = File::open(destination).map_err(|error| error.to_string())?;
            let valid_media = match role {
                AssetRole::Image => {
                    crate::media_dimensions::image_dimensions(&mut validation_file).is_some()
                }
                AssetRole::Video => {
                    crate::media_dimensions::video_dimensions(&mut validation_file).is_some()
                }
            };
            if !valid_media {
                return Err("the provider returned incomplete or unsupported media".into());
            }
            Ok(DownloadReceipt {
                sha256: hex::encode(hasher.finalize()),
                byte_count,
                content_type,
            })
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(destination);
        }
        write_result
    }
}

impl AssetRole {
    fn accepts(self, content_type: &str) -> bool {
        match self {
            Self::Image => matches!(
                content_type,
                "image/jpeg" | "image/png" | "image/webp" | "image/gif" | "image/avif"
            ),
            Self::Video => content_type == "video/mp4",
        }
    }
}

fn extension_for_content_type(content_type: &str) -> Option<&'static str> {
    match content_type {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "image/avif" => Some("avif"),
        "video/mp4" => Some("mp4"),
        _ => None,
    }
}

fn http_client(allowed: fn(&Url) -> bool, timeout: Duration) -> Result<Client, String> {
    let timeout = timeout.min(MAX_REQUEST_TIMEOUT);
    Client::builder()
        .connect_timeout(timeout.min(CONNECT_TIMEOUT))
        .timeout(timeout)
        .user_agent(concat!("Oia/", env!("CARGO_PKG_VERSION")))
        .redirect(Policy::custom(move |attempt| {
            if attempt.previous().len() >= 4 {
                attempt.stop()
            } else if allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| error.to_string())
}

fn validate_response(
    response: &reqwest::blocking::Response,
    allowed: fn(&Url) -> bool,
    limit: u64,
) -> Result<(), String> {
    if !allowed(response.url()) {
        return Err("the provider redirected to a disallowed URL".into());
    }
    if !response.status().is_success() {
        return Err(format!("the provider returned HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(format!(
            "the provider response exceeds the {limit}-byte limit"
        ));
    }
    Ok(())
}

fn read_bounded(reader: &mut impl Read, writer: &mut Vec<u8>, limit: u64) -> Result<(), String> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(());
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| "the provider response size overflowed".to_string())?;
        if total > limit {
            return Err(format!(
                "the provider response exceeds the {limit}-byte limit"
            ));
        }
        writer.extend_from_slice(&buffer[..count]);
    }
}

fn allowed_x_syndication_url(url: &Url) -> bool {
    safe_https_url(url) && url.host_str() == Some("cdn.syndication.twimg.com")
}

fn allowed_x_post_url(url: &Url) -> bool {
    safe_https_url(url)
        && url.host_str() == Some("x.com")
        && classify_x_post_url(url.as_str())
            .is_some_and(|reference| reference.canonical_url.eq_ignore_ascii_case(url.as_str()))
}

fn allowed_x_asset_url(url: &Url) -> bool {
    safe_https_url(url) && matches!(url.host_str(), Some("pbs.twimg.com" | "video.twimg.com"))
}

fn safe_https_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{import_link, parse_reading, read_metadata};

    struct FixtureRetriever {
        metadata: Vec<u8>,
        refinement: Option<Vec<u8>>,
        assets: HashMap<String, (&'static str, Vec<u8>)>,
    }

    impl SourceRetriever for FixtureRetriever {
        fn fetch_metadata(
            &self,
            url: &str,
            allowed_url: UrlPolicy,
            limit: u64,
            _: Duration,
        ) -> Result<Vec<u8>, String> {
            let parsed = Url::parse(url).map_err(|error| error.to_string())?;
            if !allowed_url(&parsed) {
                return Err("fixture metadata URL rejected".into());
            }
            let bytes = if allowed_x_post_url(&parsed) {
                self.refinement
                    .as_ref()
                    .ok_or("missing fixture public post HTML")?
            } else {
                &self.metadata
            };
            if bytes.len() as u64 > limit {
                return Err("fixture metadata exceeded limit".into());
            }
            Ok(bytes.clone())
        }

        fn download(
            &self,
            url: &str,
            destination: &Path,
            role: AssetRole,
            _: UrlPolicy,
            limit: u64,
            _: Duration,
        ) -> Result<DownloadReceipt, String> {
            let (content_type, bytes) = self
                .assets
                .get(url)
                .ok_or_else(|| format!("missing fixture asset {url}"))?;
            if bytes.len() as u64 > limit || !role.accepts(content_type) {
                return Err("fixture asset rejected".into());
            }
            fs::write(destination, bytes).map_err(|error| error.to_string())?;
            Ok(DownloadReceipt {
                sha256: crate::sha256_hex(bytes),
                byte_count: bytes.len() as u64,
                content_type: (*content_type).into(),
            })
        }
    }

    struct MutatingRetriever<'a> {
        inner: FixtureRetriever,
        during_metadata_fetch: Box<dyn Fn() + 'a>,
    }

    struct ExampleSourceProvider;

    static EXAMPLE_PROVIDER: ExampleSourceProvider = ExampleSourceProvider;

    impl SourceProvider for ExampleSourceProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                source_type: "social_post",
                provider: "example",
                site_name: "Example Social",
                metadata_url_policy: allowed_example_url,
                asset_url_policy: allowed_example_url,
            }
        }

        fn classify(&self, raw_url: &str) -> Option<SourceReference> {
            let url = Url::parse(raw_url).ok()?;
            if !safe_https_url(&url) || url.host_str() != Some("social.example") {
                return None;
            }
            let segments = url.path_segments()?.collect::<Vec<_>>();
            if segments.len() != 2 || !matches!(segments[0], "post" | "share") {
                return None;
            }
            let source_id = segments[1];
            if source_id.is_empty() || !source_id.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some(SourceReference {
                source_id: source_id.into(),
                canonical_url: format!("https://social.example/post/{source_id}"),
            })
        }

        fn metadata_url(&self, reference: &SourceReference) -> Result<String, String> {
            Ok(format!(
                "https://api.social.example/post/{}",
                reference.source_id
            ))
        }

        fn parse_metadata(
            &self,
            payload: &[u8],
            _: Option<&[u8]>,
        ) -> Result<ResolvedSource, String> {
            let source_id = std::str::from_utf8(payload)
                .map_err(|_| "example metadata was not UTF-8".to_string())?
                .trim();
            if source_id.is_empty() {
                return Err("example metadata had no source id".into());
            }
            Ok(ResolvedSource {
                source_id: source_id.into(),
                canonical_url: format!("https://social.example/post/{source_id}"),
                text: "Example post".into(),
                display_name: "Example Author".into(),
                handle: "example".into(),
                published_at: "2026-09-23T09:00:00Z".into(),
                avatar_url: "https://cdn.social.example/avatar.jpg".into(),
                attachments: vec![],
            })
        }
    }

    fn allowed_example_url(url: &Url) -> bool {
        safe_https_url(url)
            && matches!(
                url.host_str(),
                Some("api.social.example" | "cdn.social.example")
            )
    }

    impl SourceRetriever for MutatingRetriever<'_> {
        fn fetch_metadata(
            &self,
            url: &str,
            allowed_url: UrlPolicy,
            limit: u64,
            timeout: Duration,
        ) -> Result<Vec<u8>, String> {
            (self.during_metadata_fetch)();
            self.inner.fetch_metadata(url, allowed_url, limit, timeout)
        }

        fn download(
            &self,
            url: &str,
            destination: &Path,
            role: AssetRole,
            allowed_url: UrlPolicy,
            limit: u64,
            timeout: Duration,
        ) -> Result<DownloadReceipt, String> {
            self.inner
                .download(url, destination, role, allowed_url, limit, timeout)
        }
    }

    fn fixture() -> FixtureRetriever {
        FixtureRetriever {
            metadata: include_bytes!("../tests/fixtures/x-syndication-video.json").to_vec(),
            refinement: None,
            assets: HashMap::from([
                (
                    "https://pbs.twimg.com/profile_images/1595516312154869760/aSJL1rPu_normal.jpg"
                        .into(),
                    ("image/jpeg", b"avatar".to_vec()),
                ),
                (
                    "https://video.twimg.com/amplify_video/720.mp4".into(),
                    ("video/mp4", b"movie".to_vec()),
                ),
                (
                    "https://pbs.twimg.com/amplify_video_thumb/2102504201188499456/img/poster.jpg"
                        .into(),
                    ("image/jpeg", b"poster".to_vec()),
                ),
            ]),
        }
    }

    fn fixture_with_metadata(mutate: impl FnOnce(&mut serde_json::Value)) -> FixtureRetriever {
        let mut retriever = fixture();
        let mut metadata: serde_json::Value = serde_json::from_slice(&retriever.metadata).unwrap();
        mutate(&mut metadata);
        retriever.metadata = serde_json::to_vec(&metadata).unwrap();
        retriever
    }

    fn example_fixture(source_id: &str) -> FixtureRetriever {
        FixtureRetriever {
            metadata: source_id.as_bytes().to_vec(),
            refinement: None,
            assets: HashMap::new(),
        }
    }

    fn full_article(url: &str) -> SaveInput {
        SaveInput {
            quote_identity_markdown: None,
            kind: ReadingKind::Article,
            lightweight: false,
            url: url.into(),
            media_url: None,
            canonical_url: url.into(),
            title: "Post".into(),
            author: Some("Example".into()),
            site: Some("X".into()),
            saved_at: "2026-09-23T09:00:00.000Z".into(),
            markdown: "Post text".into(),
            images: vec![],
            preview_url: None,
            favicon_url: None,
            theme_color: None,
            product: None,
            link_category: None,
            excerpt: Some("Post text".into()),
            word_count: Some(2),
            lang: Some("en".into()),
        }
    }

    #[test]
    fn categorized_url_resave_returns_duplicate_without_metadata_or_artwork_fetch() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://example.com/book";
        let mut capture = full_article(url);
        capture.link_category = Some(crate::LinkCategory::Book);
        let saved = crate::save_capture(&library, capture).unwrap();
        let before = fs::read(library.article_path(&saved.id)).unwrap();
        let duplicate = save_ordinary_url_with(
            &library,
            UrlSaveRequest::new(url),
            |_| panic!("categorized duplicates must not fetch page metadata"),
            |_| panic!("categorized duplicates must not fetch artwork"),
        )
        .unwrap();
        assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
        assert_eq!(duplicate.id, saved.id);
        assert_eq!(fs::read(library.article_path(&saved.id)).unwrap(), before);
    }

    #[test]
    fn optional_metadata_and_artwork_failures_preserve_offline_route_category() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://www.waterstones.com/book/title/author/9781840919356";
        let saved = save_ordinary_url_with(
            &library,
            UrlSaveRequest::new(url),
            |_| anyhow::bail!("offline"),
            |_| anyhow::bail!("cover unavailable"),
        )
        .unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&saved.id)).unwrap()).unwrap();
        assert!(reading.metadata.lightweight);
        assert_eq!(reading.metadata.link_category.as_deref(), Some("book"));
        assert!(reading.metadata.preview_asset.is_none());
        assert!(reading.body.contains("Open link"));
    }

    #[test]
    fn one_page_snapshot_persists_category_product_price_and_local_artwork() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://example.com/book";
        let html = "<script type='application/ld+json'>{\"@type\":[\"Product\",\"Book\"],\"name\":\"Book name\",\"image\":\"/cover.png\",\"offers\":{\"price\":24,\"priceCurrency\":\"USD\"}}</script>";
        let saved = save_ordinary_url_with(
            &library,
            UrlSaveRequest::new(url),
            |_| {
                Ok(crate::DetectedPageMetadata {
                    link: crate::parse_link_metadata(html, url),
                    product: crate::parse_product_metadata(html, url),
                })
            },
            |url| {
                Ok(crate::ImageBytes {
                    url: url.into(),
                    content_type: "image/png".into(),
                    bytes: b"captured cover".to_vec(),
                })
            },
        )
        .unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&saved.id)).unwrap()).unwrap();
        assert_eq!(reading.metadata.title, "Book name");
        assert_eq!(reading.metadata.link_category.as_deref(), Some("book"));
        assert_eq!(
            reading.metadata.product.unwrap().price.as_deref(),
            Some("24")
        );
        let preview = reading.metadata.preview_asset.unwrap();
        assert_eq!(
            fs::read(library.reading_dir(&saved.id).join(preview)).unwrap(),
            b"captured cover"
        );
    }

    #[test]
    fn upgrades_an_existing_shared_url_to_a_full_social_article() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let raw_url = "https://x.com/benspringwater/status/2102505743278829840?s=12";
        let placeholder = import_link(&library, raw_url).unwrap();
        let placeholder_saved_at = read_metadata(&library.article_path(&placeholder.id))
            .unwrap()
            .saved_at;

        let outcome = save_special_url_with(
            &library,
            &UrlSaveRequest {
                url: raw_url.into(),
                title_hint: None,
                saved_at: Some("2026-09-23T09:00:00.000Z".into()),
            },
            &fixture(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Upgraded);
        assert_eq!(outcome.id, placeholder.id);
        let contents = fs::read_to_string(library.article_path(&outcome.id)).unwrap();
        let reading = parse_reading(&contents).unwrap();
        assert!(!reading.metadata.lightweight);
        assert_eq!(reading.metadata.kind, ReadingKind::Article);
        assert_eq!(reading.metadata.saved_at, placeholder_saved_at);
        let profile = reading.metadata.source_profile.unwrap();
        assert_eq!(profile.provider, "x");
        assert_eq!(profile.source_id, "2102505743278829840");
        assert_eq!(profile.attachments.len(), 1);
        assert_eq!(profile.attachments[0].kind, "video");
        assert!(library
            .reading_dir(&outcome.id)
            .join(&profile.attachments[0].asset)
            .is_file());
        assert!(reading.body.contains(ATTACHMENT_MARKER));
    }

    #[test]
    fn upgrades_a_legacy_twitter_alias_and_finds_it_through_another_share_url() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let placeholder = import_link(
            &library,
            "https://twitter.com/BenSpringwater/status/2102505743278829840?s=12&t=legacy",
        )
        .unwrap();

        let outcome = save_special_url_with(
            &library,
            &UrlSaveRequest::new(
                "https://x.com/benspringwater/status/2102505743278829840?s=46&t=current",
            ),
            &fixture(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Upgraded);
        assert_eq!(outcome.id, placeholder.id);
        assert_eq!(
            find_saved_url(
                &library,
                "https://www.twitter.com/RENAMED/status/2102505743278829840?ref_src=twsrc%5Etfw",
            )
            .unwrap()
            .as_deref(),
            Some(placeholder.id.as_str())
        );
    }

    fn legacy_post_link(library: &LibraryRoot, url: &str, canonical_url: &str) -> Metadata {
        let placeholder = import_link(library, url).unwrap();
        let path = library.article_path(&placeholder.id);
        let mut reading = parse_reading(&fs::read_to_string(path).unwrap()).unwrap();
        reading.metadata.canonical_url = canonical_url.into();
        reading.metadata.saved_at = "2021-11-08T12:34:56.000Z".into();
        reading.metadata.read_at = Some("2022-01-01T00:00:00.000Z".into());
        reading.metadata.archived = true;
        reading.metadata.favorite = true;
        reading.metadata.rating = 4;
        reading.metadata.tags = vec!["Saved inspiration".into()];
        reading.metadata.machine_tags = vec![crate::MachineTagSource {
            source: "text".into(),
            source_fingerprint: "a".repeat(64),
            analyzer_version: "1".into(),
            tags: vec!["motion".into()],
        }];
        reading.metadata.excluded_machine_tags = vec!["excluded".into()];
        reading.metadata.featured_highlight_id = Some("saved-highlight".into());
        reading.metadata.lang = Some("en".into());
        let metadata = reading.metadata.clone();
        crate::write_reading(library, reading.metadata, reading.body).unwrap();
        metadata
    }

    #[test]
    fn upgrades_media_routes_and_saved_shortlinks_in_place_without_losing_curation() {
        let canonical_url = "https://x.com/benspringwater/status/2102505743278829840";
        for url in [
            "https://twitter.com/BenSpringwater/status/2102505743278829840/photo/2?ssr=true",
            "https://t.co/odIZlOG1SS?ssr=true",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let library = LibraryRoot::new(directory.path()).unwrap();
            let before = legacy_post_link(&library, url, canonical_url);
            let reading_dir = library.reading_dir(&before.id);
            fs::create_dir_all(library.assets_dir(&before.id)).unwrap();
            for (relative, bytes) in [
                ("note.md", "Personal note"),
                ("highlights.md", "Saved passage"),
                ("assets/original.jpg", "Original thumbnail"),
            ] {
                fs::write(reading_dir.join(relative), bytes).unwrap();
            }

            let outcome = save_special_url_with(&library, &UrlSaveRequest::new(url), &fixture())
                .unwrap()
                .unwrap();
            let reading =
                parse_reading(&fs::read_to_string(library.article_path(&outcome.id)).unwrap())
                    .unwrap();
            let metadata = &reading.metadata;
            assert_eq!(outcome.disposition, SaveDisposition::Upgraded, "{url}");
            assert_eq!(outcome.id, before.id);
            assert_eq!(metadata.url, before.url);
            assert_eq!(metadata.canonical_url, canonical_url);
            assert!(!metadata.lightweight);
            assert_eq!(metadata.saved_at, before.saved_at);
            assert_eq!(metadata.read_at, before.read_at);
            assert_eq!(metadata.archived, before.archived);
            assert_eq!(metadata.favorite, before.favorite);
            assert_eq!(metadata.rating, before.rating);
            assert_eq!(metadata.tags, before.tags);
            assert_eq!(metadata.machine_tags, before.machine_tags);
            assert_eq!(metadata.excluded_machine_tags, before.excluded_machine_tags);
            assert_eq!(metadata.featured_highlight_id, before.featured_highlight_id);
            assert_eq!(metadata.lang, before.lang);
            assert_eq!(metadata.excerpt.as_deref(), Some("Motion \"study\""));
            let profile = metadata.source_profile.as_ref().unwrap();
            assert_eq!(profile.source_id, "2102505743278829840");
            assert_eq!(profile.attachments.len(), 1);
            assert_eq!(
                fs::read(reading_dir.join(&profile.attachments[0].asset)).unwrap(),
                b"movie"
            );
            assert_eq!(
                fs::read(reading_dir.join(profile.attachments[0].poster_asset.as_ref().unwrap()))
                    .unwrap(),
                b"poster"
            );
            for (relative, bytes) in [
                ("note.md", "Personal note"),
                ("highlights.md", "Saved passage"),
                ("assets/original.jpg", "Original thumbnail"),
            ] {
                assert_eq!(
                    fs::read_to_string(reading_dir.join(relative)).unwrap(),
                    bytes
                );
            }

            // Both the original link and another post alias find the upgraded
            // source. A complete re-save does not retrieve or rewrite it.
            let bytes = fs::read(library.article_path(&outcome.id)).unwrap();
            for alias in [
                url,
                "https://x.com/renamed/status/2102505743278829840/video/1",
            ] {
                assert_eq!(
                    find_saved_url(&library, alias).unwrap(),
                    Some(before.id.clone())
                );
                let duplicate = save_special_url_with(
                    &library,
                    &UrlSaveRequest::new(alias),
                    &example_fixture("must not fetch"),
                )
                .unwrap()
                .unwrap();
                assert_eq!(duplicate.disposition, SaveDisposition::Duplicate);
                assert_eq!(duplicate.id, before.id);
                assert_eq!(fs::read(library.article_path(&before.id)).unwrap(), bytes);
            }
        }
    }

    #[test]
    fn media_and_saved_shortlink_retrieval_failure_keeps_the_original_link() {
        let canonical_url = "https://x.com/benspringwater/status/2102505743278829840";
        for url in [
            "https://x.com/benspringwater/status/2102505743278829840/photo/1",
            "https://t.co/odIZlOG1SS?ssr=true",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let library = LibraryRoot::new(directory.path()).unwrap();
            let before = legacy_post_link(&library, url, canonical_url);
            let bytes = fs::read(library.article_path(&before.id)).unwrap();
            let original_asset = library.assets_dir(&before.id).join("original.jpg");
            fs::write(&original_asset, b"Original thumbnail").unwrap();
            let mut retriever = fixture();
            retriever
                .assets
                .remove("https://video.twimg.com/amplify_video/720.mp4");

            assert!(
                save_special_url_with(&library, &UrlSaveRequest::new(url), &retriever).is_err()
            );
            assert_eq!(fs::read(library.article_path(&before.id)).unwrap(), bytes);
            assert_eq!(fs::read(original_asset).unwrap(), b"Original thumbnail");
            assert_eq!(
                fs::read_dir(library.assets_dir(&before.id))
                    .unwrap()
                    .count(),
                1
            );
        }
    }

    #[test]
    fn unrelated_origins_cannot_supply_canonical_post_identity() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let canonical_url = "https://x.com/benspringwater/status/2102505743278829840";
        for url in [
            "https://example.com/copied-post",
            "https://t.co.example.com/odIZlOG1SS",
            "https://t.co/odIZlOG1SS/extra",
            "https://user:password@t.co/odIZlOG1SS",
        ] {
            let before = legacy_post_link(&library, url, canonical_url);
            assert!(
                save_special_url_with(&library, &UrlSaveRequest::new(url), &fixture())
                    .unwrap()
                    .is_none()
            );
            assert_eq!(find_saved_url(&library, canonical_url).unwrap(), None);
            assert_eq!(
                read_metadata(&library.article_path(&before.id))
                    .unwrap()
                    .source_profile,
                None
            );
        }
        assert!(save_special_url_with(
            &library,
            &UrlSaveRequest::new("https://t.co/unsavedToken"),
            &fixture(),
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn saved_shortlink_recognition_requires_a_bounded_valid_article_header() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://t.co/odIZlOG1SS?ssr=true";
        let before = legacy_post_link(
            &library,
            url,
            "https://x.com/benspringwater/status/2102505743278829840",
        );
        let path = library.article_path(&before.id);
        let contents = fs::read_to_string(&path).unwrap();
        let mut reading = parse_reading(&contents).unwrap();
        reading.metadata.lightweight = false;
        fs::write(&path, crate::render_reading(&reading).unwrap()).unwrap();
        assert!(
            save_special_url_with(&library, &UrlSaveRequest::new(url), &fixture())
                .unwrap()
                .is_none()
        );

        reading.metadata.lightweight = true;
        reading.metadata.id = "b".repeat(64);
        fs::write(&path, crate::render_reading(&reading).unwrap()).unwrap();
        assert!(
            save_special_url_with(&library, &UrlSaveRequest::new(url), &fixture())
                .unwrap()
                .is_none()
        );

        let mut oversized = b"---\n".to_vec();
        oversized.resize((SOURCE_FRONTMATTER_LIMIT + 64) as usize, b'a');
        fs::write(&path, oversized).unwrap();
        assert!(
            save_special_url_with(&library, &UrlSaveRequest::new(url), &fixture())
                .unwrap()
                .is_none()
        );
    }

    fn full_note_fixture() -> FixtureRetriever {
        let mut retriever = fixture();
        retriever.metadata =
            include_bytes!("../tests/fixtures/x-syndication-full-note.json").to_vec();
        retriever.refinement = Some(include_bytes!("../tests/fixtures/x-note-page.html").to_vec());
        retriever
    }

    #[test]
    fn long_post_refinement_upgrades_the_original_link_with_complete_text_and_media() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://twitter.com/BenSpringwater/status/2102505743278829840/photo/1";
        let canonical = "https://x.com/benspringwater/status/2102505743278829840";
        let before = legacy_post_link(&library, url, canonical);
        let outcome =
            save_special_url_with(&library, &UrlSaveRequest::new(url), &full_note_fixture())
                .unwrap()
                .unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&outcome.id)).unwrap()).unwrap();
        assert_eq!(outcome.disposition, SaveDisposition::Upgraded);
        assert_eq!(outcome.id, before.id);
        assert_eq!(reading.metadata.url, before.url);
        assert_eq!(reading.metadata.tags, before.tags);
        assert_eq!(reading.metadata.saved_at, before.saved_at);
        assert_eq!(
            reading.metadata.excerpt.as_deref(),
            Some(include_str!("../tests/fixtures/x-note-full-text.txt"))
        );
        let profile = reading.metadata.source_profile.unwrap();
        assert_eq!(
            fs::read(
                library
                    .reading_dir(&before.id)
                    .join(&profile.attachments[0].asset)
            )
            .unwrap(),
            b"movie"
        );
    }

    #[test]
    fn required_long_post_html_failures_never_promote_the_truncated_preview() {
        let url = "https://x.com/benspringwater/status/2102505743278829840";
        for refinement in [
            None,
            Some(vec![b'a'; METADATA_LIMIT as usize + 1]),
            Some(b"<html>Missing complete note</html>".to_vec()),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let library = LibraryRoot::new(directory.path()).unwrap();
            let before = legacy_post_link(&library, url, url);
            let bytes = fs::read(library.article_path(&before.id)).unwrap();
            let mut retriever = full_note_fixture();
            retriever.refinement = refinement;
            assert!(
                save_special_url_with(&library, &UrlSaveRequest::new(url), &retriever).is_err()
            );
            assert_eq!(fs::read(library.article_path(&before.id)).unwrap(), bytes);
            assert_eq!(
                fs::read_dir(library.assets_dir(&before.id))
                    .unwrap()
                    .count(),
                0
            );
        }
    }

    #[test]
    fn long_post_html_and_assets_share_one_aggregate_deadline() {
        struct SlowRefinement(FixtureRetriever);
        impl SourceRetriever for SlowRefinement {
            fn fetch_metadata(
                &self,
                url: &str,
                allowed: UrlPolicy,
                limit: u64,
                timeout: Duration,
            ) -> Result<Vec<u8>, String> {
                let result = self.0.fetch_metadata(url, allowed, limit, timeout);
                if allowed_x_post_url(&Url::parse(url).unwrap()) {
                    std::thread::sleep(Duration::from_millis(30));
                }
                result
            }
            fn download(
                &self,
                _: &str,
                _: &Path,
                _: AssetRole,
                _: UrlPolicy,
                _: u64,
                _: Duration,
            ) -> Result<DownloadReceipt, String> {
                panic!("a spent HTML retrieval deadline must prevent asset adoption")
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://x.com/benspringwater/status/2102505743278829840";
        let before = legacy_post_link(&library, url, url);
        let bytes = fs::read(library.article_path(&before.id)).unwrap();
        let error = save_special_url_with_timeout(
            &library,
            &UrlSaveRequest::new(url),
            &SlowRefinement(full_note_fixture()),
            Duration::from_millis(20),
        )
        .unwrap_err();
        assert!(
            matches!(error, SaveUrlError::Retrieval(message) if message.contains("retrieval deadline"))
        );
        assert_eq!(fs::read(library.article_path(&before.id)).unwrap(), bytes);
    }

    #[test]
    fn long_post_html_policy_allows_only_canonical_primary_post_routes() {
        for url in [
            "https://x.com/benspringwater/status/2102505743278829840",
            "https://x.com/BenSpringwater/status/2102505743278829840",
            "https://x.com/BENSPRINGWATER/status/2102505743278829840",
            "https://x.com/AnatoliKopadze/status/2070156017262793008",
        ] {
            assert!(allowed_x_post_url(&Url::parse(url).unwrap()), "{url}");
        }
        for url in [
            "https://twitter.com/benspringwater/status/2102505743278829840",
            "https://www.x.com/BenSpringwater/status/2102505743278829840",
            "https://x.com.evil.example/benspringwater/status/2102505743278829840",
            "https://x.com/benspringwater",
            "https://x.com/BenSpringwater/Status/2102505743278829840",
            "https://x.com/BenSpringwater/status/2102505743278829840/",
            "https://x.com/benspringwater/status/2102505743278829840/photo/1",
            "https://x.com/benspringwater/status/2102505743278829840?source=redirect",
            "https://x.com/BenSpringwater/status/2102505743278829840#redirect",
            "http://x.com/benspringwater/status/2102505743278829840",
            "https://user:password@x.com/benspringwater/status/2102505743278829840",
        ] {
            assert!(!allowed_x_post_url(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn new_social_save_uses_the_payloads_current_handle_and_canonical_url() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let retriever = fixture_with_metadata(|metadata| {
            metadata["user"]["screen_name"] = serde_json::json!("CurrentHandle");
        });
        let canonical_url = "https://x.com/currenthandle/status/2102505743278829840";

        let outcome = save_special_url_with(
            &library,
            &UrlSaveRequest::new(
                "https://twitter.com/FormerHandle/status/2102505743278829840?s=20",
            ),
            &retriever,
        )
        .unwrap()
        .unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Saved);
        assert_eq!(outcome.id, crate::url_id(canonical_url).unwrap());
        let metadata = read_metadata(&library.article_path(&outcome.id)).unwrap();
        assert_eq!(metadata.url, canonical_url);
        assert_eq!(metadata.canonical_url, canonical_url);
        assert_eq!(
            metadata.source_profile.unwrap().author_handle,
            "CurrentHandle"
        );
    }

    #[test]
    fn resaving_a_social_post_repairs_a_missing_attachment() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let canonical_url = "https://x.com/benspringwater/status/2102505743278829840";
        let first =
            save_special_url_with(&library, &UrlSaveRequest::new(canonical_url), &fixture())
                .unwrap()
                .unwrap();
        let metadata = read_metadata(&library.article_path(&first.id)).unwrap();
        let attachment = metadata
            .source_profile
            .unwrap()
            .attachments
            .into_iter()
            .next()
            .unwrap();
        let attachment_path = library.reading_dir(&first.id).join(&attachment.asset);
        fs::remove_file(&attachment_path).unwrap();

        let repaired = save_special_url_with(
            &library,
            &UrlSaveRequest::new(
                "https://twitter.com/BenSpringwater/status/2102505743278829840?s=12",
            ),
            &fixture(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(repaired.disposition, SaveDisposition::Upgraded);
        assert_eq!(repaired.id, first.id);
        assert_eq!(fs::read(attachment_path).unwrap(), b"movie");
    }

    #[test]
    fn social_post_text_escapes_markdown_punctuation_for_literal_rendering() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let text = concat!(
            "    leading spaces\n",
            "\tleading tab\n",
            "Setext text\n",
            "===\n",
            "1. ordered with a period\n",
            "2) ordered with a parenthesis\n",
            "Literal entities: &amp;copy; &amp;#35; &amp;amp;\n",
            "ASCII punctuation: !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"
        );
        let retriever = fixture_with_metadata(|metadata| {
            metadata["text"] = serde_json::json!(text);
            metadata
                .as_object_mut()
                .unwrap()
                .remove("display_text_range");
        });

        let outcome = save_special_url_with(
            &library,
            &UrlSaveRequest::new("https://x.com/benspringwater/status/2102505743278829840"),
            &retriever,
        )
        .unwrap()
        .unwrap();

        let contents = fs::read_to_string(library.article_path(&outcome.id)).unwrap();
        let reading = parse_reading(&contents).unwrap();
        let expected = concat!(
            "&#32;&#32;&#32;&#32;leading spaces\n",
            "&#9;leading tab\n",
            "Setext text\n",
            "\\=\\=\\=\n",
            "1\\. ordered with a period\n",
            "2\\) ordered with a parenthesis\n",
            "Literal entities\\: \\&copy\\; \\&\\#35\\; \\&amp\\;\n",
            "ASCII punctuation\\: ",
            "\\!\\\"\\#\\$\\%\\&\\'\\(\\)\\*\\+\\,\\-\\.\\/",
            "\\:\\;\\<\\=\\>\\?\\@\\[\\\\\\]\\^\\_\\`\\{\\|\\}\\~\n\n",
            "<!-- oia:attachments -->"
        );
        assert!(
            reading.body.starts_with(expected),
            "saved body did not preserve literal post text:\n{:?}",
            reading.body
        );
    }

    #[test]
    fn ordinary_links_do_not_invoke_the_provider_retriever() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let outcome =
            save_url(&library, UrlSaveRequest::new("https://example.com/article")).unwrap();
        let metadata = read_metadata(&library.article_path(&outcome.id)).unwrap();
        assert!(metadata.lightweight);
        assert!(metadata.source_profile.is_none());
    }

    #[test]
    fn provider_registry_saves_and_finds_another_provider_alias() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let shared_url = "https://social.example/share/42?from=shortcut";
        let placeholder = import_link(&library, shared_url).unwrap();
        let providers: [&dyn SourceProvider; 1] = [&EXAMPLE_PROVIDER];

        let outcome = save_special_url_with_providers(
            &library,
            &UrlSaveRequest::new(shared_url),
            &example_fixture("42"),
            &providers,
            SOURCE_RETRIEVAL_DEADLINE,
        )
        .unwrap()
        .unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Upgraded);
        assert_eq!(outcome.id, placeholder.id);
        let metadata = read_metadata(&library.article_path(&outcome.id)).unwrap();
        let profile = metadata.source_profile.unwrap();
        assert_eq!(metadata.site.as_deref(), Some("Example Social"));
        assert_eq!(profile.provider, "example");
        assert_eq!(profile.source_id, "42");
        assert_eq!(
            find_saved_url_with_providers(
                &library,
                "https://social.example/share/42?from=another-app",
                &providers,
            )
            .unwrap(),
            Some(placeholder.id)
        );
    }

    #[test]
    fn source_scan_ignores_non_utf8_article_body() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let saved = save_source_capture(
            &library,
            SourceCaptureInput {
                capture: full_article("https://twitter.com/original/status/42"),
                source_profile: SourceProfile {
                    version: 1,
                    source_type: "social_post".into(),
                    provider: "x".into(),
                    source_id: "42".into(),
                    author_handle: "original".into(),
                    published_at: None,
                    avatar_asset: None,
                    attachments: vec![],
                },
                staged_assets: vec![],
                expected_article: ExpectedArticleState::Missing,
            },
        )
        .unwrap();
        OpenOptions::new()
            .append(true)
            .open(library.article_path(&saved.id))
            .unwrap()
            .write_all(&[0xff])
            .unwrap();

        assert_eq!(
            find_saved_url(
                &library,
                "https://x.com/renamed/status/42?ref_src=another-alias",
            )
            .unwrap(),
            Some(saved.id)
        );
    }

    #[test]
    fn source_frontmatter_reader_stops_at_its_byte_limit() {
        let mut bytes = b"---\n".to_vec();
        bytes.resize((SOURCE_FRONTMATTER_LIMIT + 64) as usize, b'a');
        let mut reader = std::io::Cursor::new(bytes);

        let error = read_source_metadata_from(&mut reader).unwrap_err();

        assert!(error.to_string().contains("frontmatter exceeds"));
        assert!(reader.position() <= SOURCE_FRONTMATTER_LIMIT + 1);
    }

    #[test]
    fn source_lookup_ignores_a_profile_whose_frontmatter_id_mismatches_its_folder() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let seed = import_link(&library, "https://example.com/malformed-source-record").unwrap();
        let article = library.article_path(&seed.id);
        let mut reading = parse_reading(&fs::read_to_string(&article).unwrap()).unwrap();
        reading.metadata.id = "/é".into();
        reading.metadata.source_profile = Some(SourceProfile {
            version: 1,
            source_type: "social_post".into(),
            provider: "x".into(),
            source_id: "42".into(),
            author_handle: "example".into(),
            published_at: None,
            avatar_asset: None,
            attachments: vec![],
        });
        fs::write(&article, crate::render_reading(&reading).unwrap()).unwrap();

        let saved = find_saved_url(&library, "https://x.com/example/status/42").unwrap();

        assert_eq!(saved, None);
    }

    #[test]
    fn source_lookup_prefers_the_exact_url_identity_before_provider_aliases() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let alias = save_source_capture(
            &library,
            SourceCaptureInput {
                capture: full_article("https://example.com/copied-post"),
                source_profile: SourceProfile {
                    version: 1,
                    source_type: "social_post".into(),
                    provider: "x".into(),
                    source_id: "42".into(),
                    author_handle: "example".into(),
                    published_at: None,
                    avatar_asset: None,
                    attachments: vec![],
                },
                staged_assets: vec![],
                expected_article: ExpectedArticleState::Missing,
            },
        )
        .unwrap();
        let exact = import_link(&library, "https://x.com/example/status/42?s=12").unwrap();

        let saved = find_saved_url(&library, "https://x.com/example/status/42?s=12").unwrap();

        assert_ne!(alias.id, exact.id);
        assert_eq!(saved.as_deref(), Some(exact.id.as_str()));
    }

    #[test]
    fn source_lookup_ignores_an_unrelated_origin_with_matching_profile_identity() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        save_source_capture(
            &library,
            SourceCaptureInput {
                capture: full_article("https://example.com/copied-post"),
                source_profile: SourceProfile {
                    version: 1,
                    source_type: "social_post".into(),
                    provider: "x".into(),
                    source_id: "42".into(),
                    author_handle: "example".into(),
                    published_at: None,
                    avatar_asset: None,
                    attachments: vec![],
                },
                staged_assets: vec![],
                expected_article: ExpectedArticleState::Missing,
            },
        )
        .unwrap();

        assert_eq!(
            find_saved_url(&library, "https://x.com/example/status/42").unwrap(),
            None
        );
    }

    #[test]
    fn source_lookup_resolves_duplicate_profiles_deterministically() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let mut older_capture = full_article("https://twitter.com/older/status/42");
        older_capture.saved_at = "2026-01-01T00:00:00.000Z".into();
        let older = save_source_capture(
            &library,
            SourceCaptureInput {
                capture: older_capture,
                source_profile: SourceProfile {
                    version: 1,
                    source_type: "social_post".into(),
                    provider: "x".into(),
                    source_id: "42".into(),
                    author_handle: "older".into(),
                    published_at: None,
                    avatar_asset: None,
                    attachments: vec![],
                },
                staged_assets: vec![],
                expected_article: ExpectedArticleState::Missing,
            },
        )
        .unwrap();
        let mut newer_capture = full_article("https://x.com/newer/status/42");
        newer_capture.saved_at = "2026-02-01T00:00:00.000Z".into();
        save_source_capture(
            &library,
            SourceCaptureInput {
                capture: newer_capture,
                source_profile: SourceProfile {
                    version: 1,
                    source_type: "social_post".into(),
                    provider: "x".into(),
                    source_id: "42".into(),
                    author_handle: "newer".into(),
                    published_at: None,
                    avatar_asset: None,
                    attachments: vec![],
                },
                staged_assets: vec![],
                expected_article: ExpectedArticleState::Missing,
            },
        )
        .unwrap();

        assert_eq!(
            find_saved_url(&library, "https://www.x.com/renamed/status/42").unwrap(),
            Some(older.id)
        );
    }

    #[test]
    fn source_capture_does_not_overwrite_an_article_changed_during_metadata_fetch() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://x.com/benspringwater/status/2102505743278829840";
        let existing = crate::save_capture(&library, full_article(url)).unwrap();
        let retriever = MutatingRetriever {
            inner: fixture(),
            during_metadata_fetch: Box::new(|| {
                let contents = fs::read_to_string(library.article_path(&existing.id)).unwrap();
                let mut reading = parse_reading(&contents).unwrap();
                reading.body = "External edit during retrieval\n".into();
                reading.metadata.tags = vec!["external".into()];
                crate::write_reading(&library, reading.metadata, reading.body).unwrap();
            }),
        };

        let outcome = save_special_url_with(&library, &UrlSaveRequest::new(url), &retriever)
            .unwrap()
            .unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&existing.id)).unwrap())
                .unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Duplicate);
        assert_eq!(reading.body, "External edit during retrieval\n");
        assert_eq!(reading.metadata.tags, vec!["external"]);
        assert!(reading.metadata.source_profile.is_none());
    }

    #[test]
    fn source_capture_does_not_overwrite_an_article_created_during_metadata_fetch() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let url = "https://x.com/benspringwater/status/2102505743278829840";
        let retriever = MutatingRetriever {
            inner: fixture(),
            during_metadata_fetch: Box::new(|| {
                crate::save_capture(&library, full_article(url)).unwrap();
            }),
        };

        let outcome = save_special_url_with(&library, &UrlSaveRequest::new(url), &retriever)
            .unwrap()
            .unwrap();
        let reading =
            parse_reading(&fs::read_to_string(library.article_path(&outcome.id)).unwrap()).unwrap();

        assert_eq!(outcome.disposition, SaveDisposition::Duplicate);
        assert_eq!(reading.body, "Post text\n");
        assert!(reading.metadata.source_profile.is_none());
    }

    #[test]
    fn source_asset_completeness_rejects_per_role_and_aggregate_oversize_files() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let id = crate::url_id("https://x.com/example/status/42").unwrap();
        fs::create_dir_all(library.assets_dir(&id)).unwrap();

        let oversized_image = "a".repeat(64);
        fs::File::create(
            library
                .assets_dir(&id)
                .join(format!("{oversized_image}.jpg")),
        )
        .unwrap()
        .set_len(IMAGE_LIMIT + 1)
        .unwrap();
        let image_source = ExistingSource {
            id: id.clone(),
            url: "https://x.com/example/status/42".into(),
            profile: SourceProfile {
                version: 1,
                source_type: "social_post".into(),
                provider: "x".into(),
                source_id: "42".into(),
                author_handle: "example".into(),
                published_at: None,
                avatar_asset: None,
                attachments: vec![SourceAttachment {
                    kind: "image".into(),
                    asset: format!("assets/{oversized_image}.jpg"),
                    poster_asset: None,
                    content_type: Some("image/jpeg".into()),
                    width: None,
                    height: None,
                    alt: None,
                }],
            },
            article_sha256: "0".repeat(64),
        };
        assert!(!source_assets_complete(&library, &image_source));

        let sparse_size = TOTAL_ASSET_LIMIT / 3 + 1;
        let mut attachments = Vec::new();
        for digit in ['b', 'c', 'd'] {
            let hash = digit.to_string().repeat(64);
            fs::File::create(library.assets_dir(&id).join(format!("{hash}.mp4")))
                .unwrap()
                .set_len(sparse_size)
                .unwrap();
            attachments.push(SourceAttachment {
                kind: "video".into(),
                asset: format!("assets/{hash}.mp4"),
                poster_asset: None,
                content_type: Some("video/mp4".into()),
                width: None,
                height: None,
                alt: None,
            });
        }
        let aggregate_source = ExistingSource {
            profile: SourceProfile {
                attachments,
                ..image_source.profile.clone()
            },
            ..image_source
        };
        assert!(!source_assets_complete(&library, &aggregate_source));
    }

    #[test]
    fn source_capture_stops_when_its_aggregate_retrieval_deadline_is_exhausted() {
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();
        let request = UrlSaveRequest::new("https://x.com/example/status/42");

        let error = save_special_url_with_timeout(&library, &request, &fixture(), Duration::ZERO)
            .unwrap_err();

        assert!(matches!(
            error,
            SaveUrlError::Retrieval(message) if message.contains("retrieval deadline")
        ));
    }

    #[test]
    #[ignore = "explicit live X capture check: requires OIA_TEST_X_URL and network"]
    fn live_x_capture_downloads_a_complete_local_article() {
        let url = std::env::var("OIA_TEST_X_URL").expect("set OIA_TEST_X_URL");
        let directory = tempfile::tempdir().unwrap();
        let library = LibraryRoot::new(directory.path()).unwrap();

        let outcome = save_url(&library, UrlSaveRequest::new(url)).unwrap();
        let metadata = read_metadata(&library.article_path(&outcome.id)).unwrap();
        let profile = metadata.source_profile.expect("source profile");
        assert_eq!(metadata.kind, ReadingKind::Article);
        assert!(!metadata.lightweight);
        assert_eq!(profile.provider, "x");
        assert!(!profile.attachments.is_empty());
        for attachment in profile.attachments {
            assert!(library
                .reading_dir(&outcome.id)
                .join(attachment.asset)
                .is_file());
            if let Some(poster) = attachment.poster_asset {
                assert!(library.reading_dir(&outcome.id).join(poster).is_file());
            }
        }
    }
}
