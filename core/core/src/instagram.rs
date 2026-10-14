// SPDX-License-Identifier: MIT
//! Explicit, opt-in Instagram transport. Ordinary core imports remain offline.
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

#[derive(Debug, PartialEq, Eq)]
pub struct InstagramRequest {
    pub shortcode: String,
    pub slide: usize,
    pub origin: String,
}

impl InstagramRequest {
    pub fn parse(input: &str) -> Result<Self> {
        let url = url::Url::parse(input).context("Invalid Instagram URL.")?;
        ensure!(
            url.scheme() == "https"
                && matches!(url.host_str(), Some("instagram.com" | "www.instagram.com"))
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none(),
            "Expected an HTTPS Instagram post URL."
        );
        let segments: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
        ensure!(
            segments.len() == 3 && matches!(segments[1], "p" | "reel" | "reels"),
            "Expected an Instagram post or reel."
        );
        let shortcode = segments[2];
        ensure!(
            !shortcode.is_empty()
                && shortcode.len() <= 32
                && shortcode
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
            "Invalid Instagram post identifier."
        );
        let indices: Vec<_> = url
            .query_pairs()
            .filter(|(k, _)| k == "img_index")
            .collect();
        ensure!(indices.len() <= 1, "Ambiguous Instagram slide number.");
        let slide = if let Some((_, value)) = indices.first() {
            ensure!(
                !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()),
                "Invalid Instagram slide number."
            );
            value
                .parse::<usize>()
                .context("Invalid Instagram slide number.")?
        } else {
            1
        };
        ensure!(
            (1..=100).contains(&slide),
            "Instagram slide number is out of range."
        );
        Ok(Self {
            shortcode: shortcode.into(),
            slide,
            origin: format!("https://www.instagram.com/p/{shortcode}/?img_index={slide}"),
        })
    }
}

pub struct DownloadedMedia {
    pub directory: TempDir,
    pub name: String,
}
impl DownloadedMedia {
    pub fn path(&self) -> PathBuf {
        self.directory.path().join(&self.name)
    }
}
pub type Resolver<'a> = dyn Fn(&InstagramRequest) -> Result<DownloadedMedia> + 'a;

/// Recognize post routes before validating their selected slide. Invalid slide
/// requests must fail rather than falling through to an ordinary link save.
pub fn is_post_url(input: &str) -> bool {
    url::Url::parse(input).is_ok_and(|url| {
        matches!(url.host_str(), Some("instagram.com" | "www.instagram.com"))
            && matches!(url.path().split('/').nth(1), Some("p" | "reel" | "reels"))
    })
}

/// Explicit browser/Inbox transport; both callers use the same validated media
/// import and content-derived identity. Never substitute article text or a link.
pub fn save_with_resolver(
    library: &crate::LibraryRoot,
    request: &crate::UrlSaveRequest,
    resolver: &Resolver<'_>,
) -> Result<Option<crate::SaveOutcome>, crate::SaveUrlError> {
    if !is_post_url(&request.url) {
        return Ok(None);
    }
    let instagram = InstagramRequest::parse(&request.url)
        .map_err(|error| crate::SaveError::InvalidRequest(error.to_string()))?;
    let media =
        resolver(&instagram).map_err(|error| crate::SaveUrlError::Retrieval(error.to_string()))?;
    crate::inbox::import_instagram_media(library, &instagram, &media, request.saved_at.as_deref())
        .map(Some)
        .map_err(|error| crate::SaveUrlError::Retrieval(error.to_string()))
}

/// Paths come from the native app, never from a shared manifest.
pub fn download(
    python: &Path,
    script: &Path,
    request: &InstagramRequest,
) -> Result<DownloadedMedia> {
    ensure!(python.is_file() && script.is_file(), "Instagram downloader is not installed. Run scripts/install-instagram-downloader.sh on this Mac, then retry the save or use Check Inbox for queued shares.");
    let directory = tempfile::tempdir()?;
    let mut child = Command::new(python)
        .arg("-I")
        .arg(script)
        .arg(&request.shortcode)
        .arg(request.slide.to_string())
        .arg(directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Could not start the Instagram downloader.")?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < Duration::from_secs(120) => {
                thread::sleep(Duration::from_millis(100))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                if let Err(error) = result {
                    return Err(error.into());
                }
                bail!("Instagram download timed out. Retry the save later, or use Check Inbox for queued shares.");
            }
        }
    };
    ensure!(status.success(), "Instagram could not supply the selected media. It may require login, be unavailable, or have no such slide. Retry the save later, or use Check Inbox for queued shares.");
    let entries = fs::read_dir(directory.path())?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        entries.len() == 1,
        "Instagram did not return exactly one media file."
    );
    let name = entries[0].file_name().to_string_lossy().into_owned();
    ensure!(
        matches!(name.as_str(), "payload.jpg" | "payload.mp4"),
        "Instagram returned an unexpected file."
    );
    let metadata = fs::symlink_metadata(entries[0].path())?;
    ensure!(
        metadata.file_type().is_file()
            && metadata.len() > 0
            && metadata.len() <= crate::MAX_BROWSER_VIDEO_BYTES,
        "Instagram returned an invalid or oversized file."
    );
    Ok(DownloadedMedia { directory, name })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_slide_and_clean_origin() {
        let first =
            InstagramRequest::parse("https://www.instagram.com/p/DdlVpikk5Gj/?stkn=tracking")
                .unwrap();
        let second = InstagramRequest::parse(
            "https://www.instagram.com/p/DdlVpikk5Gj/?img_index=2&stkn=tracking",
        )
        .unwrap();
        assert_eq!(first.slide, 1);
        assert_eq!(second.slide, 2);
        assert_eq!(
            second.origin,
            "https://www.instagram.com/p/DdlVpikk5Gj/?img_index=2"
        );
        assert_eq!(
            InstagramRequest::parse("https://instagram.com/reel/abc/")
                .unwrap()
                .slide,
            1
        );
    }
    #[test]
    fn rejects_ambiguous_or_non_instagram_requests() {
        for url in [
            "https://evil.test/p/abc/",
            "https://instagram.com.evil.test/p/abc/",
            "http://instagram.com/p/abc/",
            "https://instagram.com/user/",
            "https://instagram.com/p/abc/?img_index=0",
            "https://instagram.com/p/abc/?img_index=101",
            "https://instagram.com/p/abc/?img_index=2&img_index=3",
            "https://instagram.com/p/abc/?img_index=-1",
            "https://instagram.com/p/abc/?img_index=two",
        ] {
            assert!(InstagramRequest::parse(url).is_err(), "{url}");
        }
    }
}
