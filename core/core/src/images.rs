// SPDX-License-Identifier: MIT

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write as _;

use anyhow::{bail, Context, Result};

use crate::locking::{lock_reading, ReadingLock};
use crate::types::LibraryRoot;
use crate::writer::sha256_hex;

/// One image captured by the browser extension: the URL as it appears in the
/// Markdown, the `Content-Type` the browser saw, and the raw decoded bytes.
///
/// The extension fetches images from the page (reusing the browser's cache), so
/// the core never makes a network request — it only writes what it is handed.
pub struct ImageBytes {
    pub url: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Write each supplied image into the reading's `assets/` sub-folder and rewrite
/// its URL in the Markdown to the local relative path. Returns the updated
/// Markdown.
///
/// This performs no downloads: an image the extension could not capture is
/// simply absent from `images`, so its URL stays in the Markdown and the reader
/// shows a labelled placeholder. A single image that can't be written is skipped
/// rather than failing the whole save.
pub fn write_images(
    library: &LibraryRoot,
    id: &str,
    markdown: &str,
    images: &[ImageBytes],
) -> Result<String> {
    let lock = lock_reading(library, id)?;
    write_images_under_lock(library, id, markdown, images, &lock)
}

pub(crate) fn write_images_under_lock(
    library: &LibraryRoot,
    id: &str,
    markdown: &str,
    images: &[ImageBytes],
    lock: &ReadingLock,
) -> Result<String> {
    lock.ensure_protects(library, id)?;
    let assets_dir = library.assets_dir(id);
    fs::create_dir_all(&assets_dir)?;

    let mut result = markdown.to_string();
    let mut seen = HashSet::new();
    for image in images {
        // The same URL can be supplied more than once; write it only once.
        if !seen.insert(image.url.as_str()) {
            continue;
        }
        let hash = sha256_hex(&image.bytes);
        let ext = image_extension(&image.content_type, &image.url);
        let filename = format!("{hash}.{ext}");
        if fs::write(assets_dir.join(&filename), &image.bytes).is_ok() {
            // The article file (article.md) and its assets/ folder are siblings
            // inside the reading's folder, so the link is just `assets/<file>`.
            let rel = format!("assets/{filename}");
            result = rewrite_markdown_image_destinations(&result, &image.url, &rel);
        }
    }
    Ok(result)
}

/// Write every supplied import image atomically or fail the whole operation.
///
/// Browser captures intentionally use [`write_images_under_lock`] because one
/// unavailable page image should not discard an otherwise useful article. An
/// imported image is the reading itself, so committing `article.md` without its
/// required local asset would create a permanently broken card.
pub(crate) fn write_images_required_under_lock(
    library: &LibraryRoot,
    id: &str,
    markdown: &str,
    images: &[ImageBytes],
    lock: &ReadingLock,
) -> Result<String> {
    lock.ensure_protects(library, id)?;
    let assets_dir = library.assets_dir(id);
    fs::create_dir_all(&assets_dir)?;

    let mut result = markdown.to_string();
    let mut seen = HashSet::new();
    for image in images {
        if !seen.insert(image.url.as_str()) {
            continue;
        }
        let hash = sha256_hex(&image.bytes);
        let ext = image_extension(&image.content_type, &image.url);
        let filename = format!("{hash}.{ext}");
        write_required_asset(&assets_dir, &filename, &hash, &image.bytes)?;
        result =
            rewrite_markdown_image_destinations(&result, &image.url, &format!("assets/{filename}"));
    }
    Ok(result)
}

fn write_required_asset(
    assets_dir: &std::path::Path,
    filename: &str,
    expected_hash: &str,
    bytes: &[u8],
) -> Result<()> {
    let destination = assets_dir.join(filename);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_file() => {
            let existing = fs::read(&destination)?;
            if sha256_hex(&existing) == expected_hash {
                return Ok(());
            }
            bail!(
                "existing imported image asset has unexpected contents: {}",
                destination.display()
            );
        }
        Ok(_) => bail!(
            "imported image asset path is not a regular file: {}",
            destination.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let temp_path = assets_dir.join(format!(".asset.{}.tmp", crate::new_id()));
    // Do not clean this path if exclusive creation itself fails: in that case
    // it is not our temporary file. Once creation succeeds, every error path
    // below removes only this unique sibling and never the final destination.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .with_context(|| format!("could not create image asset temporary file for {filename}"))?;
    let write_result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, &destination)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result.with_context(|| format!("could not save required image asset {filename}"))
}

/// Return the first local image target in rewritten Markdown.
///
/// `write_images` emits targets in the constrained form `assets/<file>`, so a
/// small scanner is sufficient here and avoids treating ordinary links as
/// previews. Remote images that could not be captured are skipped.
pub fn first_local_image_asset(markdown: &str) -> Option<String> {
    let mut remaining = markdown;
    while let Some(image_start) = remaining.find("![") {
        let after_marker = &remaining[image_start + 2..];
        let destination_start = after_marker.find("](")?;
        let destination_and_rest = &after_marker[destination_start + 2..];
        let destination_end = destination_and_rest.find(')')?;

        let raw = destination_and_rest[..destination_end].trim();
        let target = raw
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or_else(|| raw.split_ascii_whitespace().next().unwrap_or(""));
        if is_local_asset_path(target) {
            return Some(target.to_string());
        }

        remaining = &destination_and_rest[destination_end + 1..];
    }
    None
}

/// Resolve the local asset path written for one captured source URL.
///
/// Browser previews and favicons need a durable frontmatter reference without
/// being inserted into the visible Markdown body. The writer's file naming is
/// content-addressed, so the expected path can be derived and then verified.
pub(crate) fn written_image_asset(
    library: &LibraryRoot,
    id: &str,
    images: &[ImageBytes],
    source_url: &str,
) -> Option<String> {
    let image = images.iter().find(|image| image.url == source_url)?;
    let hash = sha256_hex(&image.bytes);
    let extension = image_extension(&image.content_type, &image.url);
    let relative = format!("assets/{hash}.{extension}");
    library
        .reading_dir(id)
        .join(&relative)
        .is_file()
        .then_some(relative)
}

fn is_local_asset_path(path: &str) -> bool {
    path.strip_prefix("assets/")
        .is_some_and(|file| !file.is_empty() && !file.contains('/') && file != "." && file != "..")
}

/// Rewrite only complete inline-image destinations that match `source_url`.
///
/// Captured social previews and favicons share the image transport but do not
/// belong in the body. A global string replacement would corrupt ordinary
/// links, prose, or a different body URL for which the role URL is a prefix.
pub(crate) fn rewrite_markdown_image_destinations(
    markdown: &str,
    source_url: &str,
    replacement: &str,
) -> String {
    if source_url.is_empty() {
        return markdown.to_string();
    }

    let bytes = markdown.as_bytes();
    let mut matches = Vec::new();
    let mut cursor = 0;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] != b'!' || bytes[cursor + 1] != b'[' || is_markdown_escaped(bytes, cursor)
        {
            cursor += 1;
            continue;
        }

        let mut alt_cursor = cursor + 2;
        let mut bracket_depth = 1usize;
        let mut alt_end = None;
        while alt_cursor < bytes.len() {
            match bytes[alt_cursor] {
                b'\\' => alt_cursor = (alt_cursor + 2).min(bytes.len()),
                b'[' => {
                    bracket_depth += 1;
                    alt_cursor += 1;
                }
                b']' => {
                    bracket_depth -= 1;
                    if bracket_depth == 0 {
                        alt_end = Some(alt_cursor);
                        break;
                    }
                    alt_cursor += 1;
                }
                _ => alt_cursor += 1,
            }
        }

        let Some(alt_end) = alt_end else {
            break;
        };
        if bytes.get(alt_end + 1) != Some(&b'(') {
            cursor = alt_end + 1;
            continue;
        }

        let mut destination_cursor = alt_end + 2;
        while bytes
            .get(destination_cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            destination_cursor += 1;
        }
        if destination_cursor >= bytes.len() {
            break;
        }

        let (destination_start, destination_end) = if bytes[destination_cursor] == b'<' {
            let start = destination_cursor + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'>' {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            if end >= bytes.len() {
                cursor = destination_cursor + 1;
                continue;
            }
            (start, end)
        } else {
            let start = destination_cursor;
            let mut end = start;
            let mut parentheses = 0usize;
            while end < bytes.len() {
                match bytes[end] {
                    b'\\' => end = (end + 2).min(bytes.len()),
                    b'(' => {
                        parentheses += 1;
                        end += 1;
                    }
                    b')' if parentheses == 0 => break,
                    b')' => {
                        parentheses -= 1;
                        end += 1;
                    }
                    byte if byte.is_ascii_whitespace() && parentheses == 0 => break,
                    _ => end += 1,
                }
            }
            (start, end)
        };

        if markdown.get(destination_start..destination_end) == Some(source_url) {
            matches.push((destination_start, destination_end));
        }
        cursor = destination_end.max(cursor + 2);
    }

    if matches.is_empty() {
        return markdown.to_string();
    }

    let mut rewritten = String::with_capacity(markdown.len());
    let mut copied_through = 0;
    for (start, end) in matches {
        rewritten.push_str(&markdown[copied_through..start]);
        rewritten.push_str(replacement);
        copied_through = end;
    }
    rewritten.push_str(&markdown[copied_through..]);
    rewritten
}

fn is_markdown_escaped(bytes: &[u8], index: usize) -> bool {
    let mut preceding_backslashes = 0;
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        preceding_backslashes += 1;
        cursor -= 1;
    }
    preceding_backslashes % 2 == 1
}

/// Choose a file extension from the `Content-Type`, falling back to the URL's
/// own extension, then `bin`.
pub(crate) fn image_extension(content_type: &str, url: &str) -> String {
    content_type_to_ext(content_type)
        .map(str::to_string)
        .or_else(|| url_ext(url))
        .unwrap_or_else(|| "bin".to_string())
}

fn content_type_to_ext(ct: &str) -> Option<&'static str> {
    match ct.split(';').next()?.trim() {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        "image/avif" => Some("avif"),
        "image/heic" | "image/heic-sequence" => Some("heic"),
        "image/heif" | "image/heif-sequence" => Some("heif"),
        "image/tiff" => Some("tiff"),
        "image/bmp" => Some("bmp"),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico"),
        "image/jp2" => Some("jp2"),
        "image/jxl" => Some("jxl"),
        _ => None,
    }
}

fn url_ext(url: &str) -> Option<String> {
    let path = url.split('?').next()?;
    let filename = path.rsplit('/').next()?;
    // rsplitn(2) gives [ext, stem]; if there's no dot we only get one part → None
    let mut parts = filename.rsplitn(2, '.');
    let ext = parts.next()?;
    parts.next()?; // require a stem before the dot
    if ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(ext.to_ascii_lowercase())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(url: &str, content_type: &str, bytes: &[u8]) -> ImageBytes {
        ImageBytes {
            url: url.to_string(),
            content_type: content_type.to_string(),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn url_ext_extracts_extension() {
        assert_eq!(
            url_ext("https://example.com/img/photo.jpg"),
            Some("jpg".to_string())
        );
        assert_eq!(
            url_ext("https://example.com/img/photo.JPG"),
            Some("jpg".to_string())
        );
        assert_eq!(
            url_ext("https://example.com/img/photo.jpg?size=large"),
            Some("jpg".to_string())
        );
        assert_eq!(url_ext("https://example.com/image"), None);
    }

    #[test]
    fn content_type_mapping() {
        assert_eq!(content_type_to_ext("image/jpeg"), Some("jpg"));
        assert_eq!(content_type_to_ext("image/png"), Some("png"));
        assert_eq!(content_type_to_ext("image/heic"), Some("heic"));
        assert_eq!(content_type_to_ext("image/heif"), Some("heif"));
        assert_eq!(content_type_to_ext("image/tiff"), Some("tiff"));
        assert_eq!(content_type_to_ext("image/bmp"), Some("bmp"));
        assert_eq!(content_type_to_ext("image/x-icon"), Some("ico"));
        assert_eq!(content_type_to_ext("image/jp2"), Some("jp2"));
        assert_eq!(content_type_to_ext("image/jxl"), Some("jxl"));
        assert_eq!(content_type_to_ext("image/vnd.microsoft.icon"), Some("ico"));
        assert_eq!(
            content_type_to_ext("image/jpeg; charset=utf-8"),
            Some("jpg")
        );
        assert_eq!(content_type_to_ext("text/html"), None);
    }

    #[test]
    fn ext_prefers_content_type_then_url_then_bin() {
        assert_eq!(image_extension("image/png", "https://e.com/x"), "png");
        assert_eq!(image_extension("", "https://e.com/x.gif"), "gif");
        assert_eq!(
            image_extension("application/octet-stream", "https://e.com/x"),
            "bin"
        );
    }

    #[test]
    fn rewrites_only_exact_inline_image_destinations() {
        let source = "https://cdn.example.com/social.png";
        let markdown = format!(
            "[ordinary link]({source})\n\n![variant]({source}?width=640)\n\n![exact](<{source}>)"
        );

        let rewritten = rewrite_markdown_image_destinations(&markdown, source, "assets/social.png");

        assert!(rewritten.contains(&format!("[ordinary link]({source})")));
        assert!(rewritten.contains(&format!("![variant]({source}?width=640)")));
        assert!(rewritten.contains("![exact](<assets/social.png>)"));
    }

    #[test]
    fn writes_asset_and_rewrites_markdown() {
        let dir = tempfile::TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let id = "TESTID";
        let url = "https://e.com/a.png";
        let bytes = b"\x89PNG\r\n\x1a\npixels";
        let markdown = format!("![alt]({url})");

        let out = write_images(&library, id, &markdown, &[img(url, "image/png", bytes)]).unwrap();

        assert!(!out.contains(url), "remote URL should be replaced: {out}");
        assert!(out.contains("assets/"), "got: {out}");
        assert!(
            !out.contains("../"),
            "link is relative to the article file (no ../): {out}"
        );

        // The written file's name is the sha256 of the bytes plus the extension.
        let expected = format!("{}.png", sha256_hex(bytes));
        let path = library.assets_dir(id).join(&expected);
        assert!(path.exists(), "asset {expected} not written");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn duplicate_url_is_written_once() {
        let dir = tempfile::TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let id = "DUP";
        let url = "https://e.com/a.png";
        let bytes = b"samebytes";
        let images = vec![img(url, "image/png", bytes), img(url, "image/png", bytes)];

        write_images(&library, id, &format!("![a]({url})"), &images).unwrap();

        let count = fs::read_dir(library.assets_dir(id)).unwrap().count();
        assert_eq!(count, 1, "the repeated URL should produce one asset");
    }

    #[test]
    fn missing_image_keeps_remote_url_as_placeholder() {
        let dir = tempfile::TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let id = "MISS";
        let url = "https://e.com/not-supplied.png";
        let markdown = format!("![a]({url})");

        // No bytes supplied for the URL → it stays remote.
        let out = write_images(&library, id, &markdown, &[]).unwrap();
        assert_eq!(out, markdown);
    }

    #[test]
    fn first_local_image_asset_skips_remote_images() {
        let markdown = "![remote](https://example.com/a.jpg)\n![local](assets/hash.png)";
        assert_eq!(
            first_local_image_asset(markdown).as_deref(),
            Some("assets/hash.png")
        );
    }

    #[test]
    fn first_local_image_asset_ignores_non_image_links_and_escaping_paths() {
        let markdown = "[asset](assets/file.png)\n![bad](assets/../secret.png)";
        assert_eq!(first_local_image_asset(markdown), None);
    }
}
