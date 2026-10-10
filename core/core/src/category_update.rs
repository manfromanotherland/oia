// SPDX-License-Identifier: MIT

//! Recoverable category enrichment without rewriting a saved reading.

use std::{
    fs,
    io::{Read, Write},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use rustix::fs::{AtFlags, Mode, OFlags};

use crate::{ImageBytes, LibraryRoot, LinkCategory, ReadingKind};

const MAX_ARTWORK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct CategoryUpdateOutcome {
    pub changed: bool,
    /// A new immutable file created by this operation, relative to the reading.
    pub created_asset: Option<String>,
    pub asset_sha256: Option<String>,
}

/// Fetch optional card artwork through the same bounded public-URL path as page
/// metadata. HTML, SVG, oversized images, and unsupported formats are rejected.
pub fn fetch_category_artwork(url: &str) -> Result<ImageBytes> {
    let (bytes, _, final_url) =
        crate::product::fetch_public_bytes(url, MAX_ARTWORK_BYTES, Duration::from_secs(8))?;
    let (_, content_type) = artwork_type(&bytes)?;
    Ok(ImageBytes {
        url: final_url,
        content_type: content_type.into(),
        bytes,
    })
}

fn artwork_type(bytes: &[u8]) -> Result<(&'static str, &'static str)> {
    if bytes.is_empty() || bytes.len() > MAX_ARTWORK_BYTES {
        bail!("category artwork exceeded size limit");
    }
    let format = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("png", "image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        ("jpg", "image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        ("gif", "image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        ("webp", "image/webp")
    } else if bytes.get(4..8) == Some(b"ftyp")
        && bytes
            .get(8..32)
            .is_some_and(|brands| brands.windows(4).any(|b| b == b"avif" || b == b"avis"))
    {
        ("avif", "image/avif")
    } else {
        bail!("unsupported category artwork format");
    };
    let size = imagesize::blob_size(bytes).context("invalid category artwork")?;
    if size.width == 0
        || size.height == 0
        || size.width > 20_000
        || size.height > 20_000
        || size.width.saturating_mul(size.height) > 100_000_000
    {
        bail!("category artwork dimensions exceeded limit");
    }
    Ok(format)
}

pub(crate) fn category_artwork_is_supported(bytes: &[u8]) -> bool {
    artwork_type(bytes).is_ok()
}

/// Insert a category and, only when no preview is recorded, optional local
/// artwork. The full article snapshot is checked under its reading lock and
/// immediately before the atomic article commit. All other frontmatter and the
/// raw Markdown body remain byte-for-byte intact, including product prices.
pub fn update_link_category_if_unchanged(
    library: &LibraryRoot,
    id: &str,
    category: LinkCategory,
    expected_sha256: &str,
    artwork: Option<ImageBytes>,
) -> Result<CategoryUpdateOutcome> {
    update_link_category_with_hook(library, id, category, expected_sha256, artwork, || {})
}

fn update_link_category_with_hook(
    library: &LibraryRoot,
    id: &str,
    category: LinkCategory,
    expected_sha256: &str,
    artwork: Option<ImageBytes>,
    before_commit: impl FnOnce(),
) -> Result<CategoryUpdateOutcome> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || expected_sha256.len() != 64
        || !expected_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("invalid category migration snapshot");
    }
    let lock = crate::locking::lock_reading(library, id)?;
    let dir = crate::notes::open_reading_dir_components(library, id)?;
    let (original, permissions) = read_article(&dir)?;
    if crate::sha256_hex(&original) != expected_sha256 {
        return Ok(CategoryUpdateOutcome::default());
    }
    let text = std::str::from_utf8(&original)?;
    let reading = crate::parse_reading(text)?;
    let metadata = reading.metadata;
    if metadata.id != id
        || metadata.kind != ReadingKind::Article
        || metadata
            .source_profile
            .as_ref()
            .is_some_and(|source| source.source_type == "social_post")
        || metadata.link_category.is_some()
        || crate::url_id(&metadata.url).ok().as_deref() != Some(id)
    {
        return Ok(CategoryUpdateOutcome::default());
    }
    let artwork = if metadata
        .preview_asset
        .as_deref()
        .is_some_and(|path| usable_preview(&dir, path))
    {
        None
    } else {
        artwork
    };
    // Validate the surgical header format before creating any new asset.
    patch_category(text, category.as_str(), None)?;
    let mut outcome = CategoryUpdateOutcome::default();
    let mut installed_assets = None;
    let preview = if let Some(artwork) = artwork {
        let (extension, _) = artwork_type(&artwork.bytes)?;
        let hash = crate::sha256_hex(&artwork.bytes);
        let filename = format!("category-artwork-{hash}.{extension}");
        let assets = open_assets_dir(&dir)?;
        let created = install_immutable_asset(&assets, &filename, &artwork.bytes, &hash)?;
        let relative = format!("assets/{filename}");
        if created {
            outcome.created_asset = Some(relative.clone());
        }
        outcome.asset_sha256 = Some(hash);
        installed_assets = Some(assets);
        Some(relative)
    } else {
        None
    };
    let result = patch_category(text, category.as_str(), preview.as_deref())?;
    before_commit();
    let temp_name = format!(".category.{}.tmp", crate::new_id());
    let write_result = (|| -> Result<bool> {
        lock.ensure_protects(library, id)?;
        let fd = rustix::fs::openat(
            &dir,
            temp_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )?;
        let mut file = fs::File::from(fd);
        file.set_permissions(permissions)?;
        file.write_all(result.as_bytes())?;
        file.sync_all()?;
        drop(file);
        // Pinning the directory prevents symlink traversal; checking its current
        // identity also avoids committing into an externally replaced folder.
        let current_dir = crate::notes::open_reading_dir_components(library, id)?;
        let expected_stat = rustix::fs::fstat(&dir)?;
        let current_stat = rustix::fs::fstat(&current_dir)?;
        if expected_stat.st_dev != current_stat.st_dev
            || expected_stat.st_ino != current_stat.st_ino
            || crate::sha256_hex(&read_article(&dir)?.0) != expected_sha256
        {
            return Ok(false);
        }
        if let (Some(assets), Some(preview), Some(hash)) = (
            installed_assets.as_ref(),
            preview.as_deref(),
            outcome.asset_sha256.as_deref(),
        ) {
            let Ok(current_assets) = rustix::fs::openat(
                &dir,
                "assets",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            ) else {
                return Ok(false);
            };
            let original_stat = rustix::fs::fstat(assets)?;
            let current_stat = rustix::fs::fstat(&current_assets)?;
            let name = preview
                .strip_prefix("assets/")
                .context("invalid staged category artwork")?;
            if original_stat.st_dev != current_stat.st_dev
                || original_stat.st_ino != current_stat.st_ino
                || !read_asset(&current_assets, name)
                    .is_ok_and(|bytes| crate::sha256_hex(&bytes) == hash)
            {
                return Ok(false);
            }
        }
        rustix::fs::renameat(&dir, temp_name.as_str(), &dir, "article.md")?;
        rustix::fs::fsync(&dir)?;
        Ok(true)
    })();
    let _ = rustix::fs::unlinkat(&dir, temp_name.as_str(), AtFlags::empty());
    match write_result {
        Ok(true) => {
            outcome.changed = true;
            Ok(outcome)
        }
        // A concurrent writer may already reference an installed asset. Keep
        // immutable artwork on conflicts; migration reports record extra files.
        Ok(false) => Ok(outcome),
        Err(error) => Err(error),
    }
}

fn read_article(dir: &rustix::fd::OwnedFd) -> Result<(Vec<u8>, fs::Permissions)> {
    let fd = rustix::fs::openat(
        dir,
        "article.md",
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let mut file = fs::File::from(fd);
    let stat = file.metadata()?;
    if !stat.is_file() {
        bail!("category article is not a regular file");
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((bytes, stat.permissions()))
}

fn open_assets_dir(dir: &rustix::fd::OwnedFd) -> Result<rustix::fd::OwnedFd> {
    match rustix::fs::mkdirat(
        dir,
        "assets",
        Mode::RUSR | Mode::WUSR | Mode::XUSR | Mode::RGRP | Mode::XGRP | Mode::ROTH | Mode::XOTH,
    ) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(error.into()),
    }
    Ok(rustix::fs::openat(
        dir,
        "assets",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?)
}

fn read_asset(dir: &rustix::fd::OwnedFd, name: &str) -> Result<Vec<u8>> {
    let fd = rustix::fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let file = fs::File::from(fd);
    if !file.metadata()?.is_file() {
        bail!("category artwork path is not a regular file");
    }
    let mut bytes = Vec::new();
    file.take((MAX_ARTWORK_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn usable_preview(dir: &rustix::fd::OwnedFd, relative: &str) -> bool {
    let Some(name) = relative.strip_prefix("assets/") else {
        return false;
    };
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return false;
    }
    let Ok(assets) = rustix::fs::openat(
        dir,
        "assets",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) else {
        return false;
    };
    let Ok(fd) = rustix::fs::openat(
        &assets,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) else {
        return false;
    };
    fs::File::from(fd)
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

fn install_immutable_asset(
    dir: &rustix::fd::OwnedFd,
    name: &str,
    bytes: &[u8],
    hash: &str,
) -> Result<bool> {
    let temp = format!(".category-artwork.{}.tmp", crate::new_id());
    let fd = rustix::fs::openat(
        dir,
        temp.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
    )?;
    let result = (|| -> Result<bool> {
        let mut file = fs::File::from(fd);
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // linkat fails if the destination exists, unlike rename, so a file
        // created by a concurrent external writer can never be replaced.
        match rustix::fs::linkat(dir, temp.as_str(), dir, name, AtFlags::empty()) {
            Ok(()) => {
                rustix::fs::fsync(dir)?;
                Ok(true)
            }
            Err(rustix::io::Errno::EXIST) if crate::sha256_hex(&read_asset(dir, name)?) == hash => {
                Ok(false)
            }
            Err(error) => Err(error.into()),
        }
    })();
    let _ = rustix::fs::unlinkat(dir, temp.as_str(), AtFlags::empty());
    result
}

fn patch_category(original: &str, category: &str, preview: Option<&str>) -> Result<String> {
    let rest = original
        .strip_prefix("---\n")
        .context("missing category frontmatter")?;
    let close = rest
        .find("\n---\n")
        .context("missing category frontmatter close")?;
    let mut retained = String::new();
    let mut skipping = false;
    for line in rest[..close].split_inclusive('\n') {
        let top = !line.starts_with([' ', '\t']) && !line.trim().is_empty();
        if top && !line.starts_with('#') {
            skipping = line.split_once(':').is_some_and(|(key, _)| {
                let key = key.trim().trim_matches(['\'', '"']);
                key == "link_category" || (preview.is_some() && key == "preview_asset")
            });
        }
        if !skipping || line.trim_start().starts_with('#') || line.trim().is_empty() {
            retained.push_str(line);
        }
    }
    if !retained.ends_with('\n') {
        retained.push('\n');
    }
    retained.push_str(&format!("link_category: {category}\n"));
    if let Some(preview) = preview {
        retained.push_str(&format!("preview_asset: {preview}\n"));
    }
    Ok(format!("---\n{retained}{}", &rest[close + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{import_link, parse_reading, sha256_hex};
    use tempfile::TempDir;

    fn png() -> Vec<u8> {
        hex::decode("89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000b49444154789c636000020000050001a5f645400000000049454e44ae426082").unwrap()
    }

    fn artwork() -> ImageBytes {
        ImageBytes {
            url: "https://example.com/cover".into(),
            content_type: "text/html".into(),
            bytes: png(),
        }
    }

    fn saved() -> (TempDir, LibraryRoot, String) {
        let dir = TempDir::new().unwrap();
        let library = LibraryRoot::new(dir.path()).unwrap();
        let outcome = import_link(&library, "https://example.com/book").unwrap();
        (dir, library, outcome.id)
    }

    #[test]
    fn category_and_artwork_preserve_raw_user_data_and_product_prices() {
        let (_dir, library, id) = saved();
        let path = library.article_path(&id);
        let original = fs::read_to_string(&path).unwrap();
        let original = original.replacen("\n---\n", "\n# external comment\ncustom_key:\n  arbitrary: true\nproduct:\n  version: 1\n  price: '140'\n  currency: EUR\n  price_is_from: false\n---\n", 1);
        fs::write(&path, &original).unwrap();
        let original_reading = parse_reading(&original).unwrap();
        let outcome = update_link_category_if_unchanged(
            &library,
            &id,
            LinkCategory::Book,
            &sha256_hex(original.as_bytes()),
            Some(artwork()),
        )
        .unwrap();
        assert!(outcome.changed);
        let updated = fs::read_to_string(&path).unwrap();
        let mut updated_reading = parse_reading(&updated).unwrap();
        assert_eq!(updated_reading.body, original_reading.body);
        assert_eq!(
            updated_reading.metadata.link_category.as_deref(),
            Some("book")
        );
        let asset = updated_reading.metadata.preview_asset.take().unwrap();
        assert_eq!(outcome.created_asset.as_deref(), Some(asset.as_str()));
        assert_eq!(
            fs::read(library.reading_dir(&id).join(&asset)).unwrap(),
            png()
        );
        updated_reading.metadata.link_category = None;
        assert_eq!(updated_reading.metadata, original_reading.metadata);
        let restored = updated
            .lines()
            .filter(|line| {
                !line.starts_with("link_category:") && !line.starts_with("preview_asset:")
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(restored, original.trim_end_matches('\n'));
        assert!(
            !update_link_category_if_unchanged(
                &library,
                &id,
                LinkCategory::Music,
                &sha256_hex(updated.as_bytes()),
                Some(artwork())
            )
            .unwrap()
            .changed
        );
    }

    #[test]
    fn existing_preview_assets_are_never_replaced() {
        let (_dir, library, id) = saved();
        let path = library.article_path(&id);
        let original = fs::read_to_string(&path).unwrap().replacen(
            "\n---\n",
            "\npreview_asset: assets/existing.png\n---\n",
            1,
        );
        let asset = library.assets_dir(&id).join("existing.png");
        fs::write(&asset, b"keep original artwork").unwrap();
        fs::write(&path, &original).unwrap();
        let result = update_link_category_if_unchanged(
            &library,
            &id,
            LinkCategory::Music,
            &sha256_hex(original.as_bytes()),
            Some(artwork()),
        )
        .unwrap();
        assert!(result.changed);
        assert!(result.created_asset.is_none());
        assert_eq!(fs::read(&asset).unwrap(), b"keep original artwork");
        assert_eq!(
            parse_reading(&fs::read_to_string(path).unwrap())
                .unwrap()
                .metadata
                .preview_asset
                .as_deref(),
            Some("assets/existing.png")
        );
        assert_eq!(fs::read_dir(library.assets_dir(&id)).unwrap().count(), 1);
    }

    #[test]
    fn stale_snapshot_cannot_create_assets_or_change_the_article() {
        let (_dir, library, id) = saved();
        let path = library.article_path(&id);
        let snapshot = fs::read(&path).unwrap();
        let mut edited = snapshot.clone();
        edited.extend_from_slice(b"\nExternal writer's text.\n");
        fs::write(&path, &edited).unwrap();
        assert!(
            !update_link_category_if_unchanged(
                &library,
                &id,
                LinkCategory::Movie,
                &sha256_hex(&snapshot),
                Some(artwork())
            )
            .unwrap()
            .changed
        );
        assert_eq!(fs::read(&path).unwrap(), edited);
        assert_eq!(fs::read_dir(library.assets_dir(&id)).unwrap().count(), 0);
    }

    #[test]
    fn missing_preview_reference_gets_new_artwork_without_replacing_any_old_asset() {
        let (_dir, library, id) = saved();
        let path = library.article_path(&id);
        let original = fs::read_to_string(&path).unwrap().replacen(
            "\n---\n",
            "\npreview_asset: assets/missing.png\n---\n",
            1,
        );
        fs::write(&path, &original).unwrap();
        fs::write(
            library.assets_dir(&id).join("unrelated.png"),
            "Keep original",
        )
        .unwrap();
        let outcome = update_link_category_if_unchanged(
            &library,
            &id,
            LinkCategory::Book,
            &sha256_hex(original.as_bytes()),
            Some(artwork()),
        )
        .unwrap();
        assert!(outcome.changed);
        let updated = parse_reading(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(updated.metadata.preview_asset, outcome.created_asset);
        assert_eq!(
            fs::read(library.assets_dir(&id).join("unrelated.png")).unwrap(),
            b"Keep original"
        );
        assert!(!library.assets_dir(&id).join("missing.png").exists());
    }

    #[test]
    fn replacing_null_preview_preserves_indented_comments_and_blank_lines() {
        let (_dir, library, id) = saved();
        let path = library.article_path(&id);
        let original = fs::read_to_string(&path).unwrap().replacen("\n---\n",
            "\npreview_asset: null\n  # preserve exporter note\n\n    # preserve deeper comment\ncustom_key: keep\n---\n", 1);
        fs::write(&path, &original).unwrap();
        let outcome = update_link_category_if_unchanged(
            &library,
            &id,
            LinkCategory::Book,
            &sha256_hex(original.as_bytes()),
            Some(artwork()),
        )
        .unwrap();
        assert!(outcome.changed);
        let updated = fs::read_to_string(&path).unwrap();
        let without_fields = |text: &str| {
            text.lines()
                .filter(|line| {
                    !line.starts_with("preview_asset:") && !line.starts_with("link_category:")
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(without_fields(&updated), without_fields(&original));
        assert_eq!(
            parse_reading(&updated).unwrap().body,
            parse_reading(&original).unwrap().body
        );
    }

    #[test]
    fn external_writer_can_reference_new_artwork_without_losing_it_on_conflict() {
        let (_dir, library, id) = saved();
        let path = library.article_path(&id);
        let original = fs::read_to_string(&path).unwrap();
        let asset = format!("assets/category-artwork-{}.png", sha256_hex(&png()));
        let edited = format!("{original}\n![External writer's cover]({asset})\n");
        let outcome = update_link_category_with_hook(
            &library,
            &id,
            LinkCategory::Book,
            &sha256_hex(original.as_bytes()),
            Some(artwork()),
            || fs::write(&path, &edited).unwrap(),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(outcome.created_asset.as_deref(), Some(asset.as_str()));
        assert_eq!(fs::read_to_string(&path).unwrap(), edited);
        assert_eq!(
            fs::read(library.reading_dir(&id).join(asset)).unwrap(),
            png()
        );
        assert!(parse_reading(&edited)
            .unwrap()
            .metadata
            .link_category
            .is_none());
    }

    #[test]
    fn externally_replaced_assets_directory_cannot_commit_a_dangling_preview() {
        let (_dir, library, id) = saved();
        let path = library.article_path(&id);
        let original = fs::read_to_string(&path).unwrap();
        let old_assets = library.reading_dir(&id).join("old-assets");
        let outcome = update_link_category_with_hook(
            &library,
            &id,
            LinkCategory::Book,
            &sha256_hex(original.as_bytes()),
            Some(artwork()),
            || {
                fs::rename(library.assets_dir(&id), &old_assets).unwrap();
                fs::create_dir(library.assets_dir(&id)).unwrap();
            },
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(fs::read_dir(library.assets_dir(&id)).unwrap().count(), 0);
        let filename = outcome
            .created_asset
            .unwrap()
            .trim_start_matches("assets/")
            .to_owned();
        assert_eq!(fs::read(old_assets.join(filename)).unwrap(), png());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_article_or_assets_are_rejected_without_touching_the_target() {
        use std::os::unix::fs::symlink;
        let (_dir, library, id) = saved();
        let outside = TempDir::new().unwrap();
        let external = outside.path().join("article.md");
        let original = fs::read(library.article_path(&id)).unwrap();
        fs::write(&external, &original).unwrap();
        fs::remove_file(library.article_path(&id)).unwrap();
        symlink(&external, library.article_path(&id)).unwrap();
        assert!(update_link_category_if_unchanged(
            &library,
            &id,
            LinkCategory::Book,
            &sha256_hex(&original),
            None
        )
        .is_err());
        assert_eq!(fs::read(&external).unwrap(), original);
        fs::remove_file(library.article_path(&id)).unwrap();
        fs::write(library.article_path(&id), &original).unwrap();
        fs::remove_dir(library.assets_dir(&id)).unwrap();
        symlink(outside.path(), library.assets_dir(&id)).unwrap();
        assert!(update_link_category_if_unchanged(
            &library,
            &id,
            LinkCategory::Book,
            &sha256_hex(&original),
            Some(artwork())
        )
        .is_err());
        assert_eq!(fs::read(library.article_path(&id)).unwrap(), original);
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
    }

    #[test]
    fn rejects_markup_disguised_as_artwork_and_truncated_images() {
        assert!(artwork_type(b"<svg xmlns='http://www.w3.org/2000/svg'>").is_err());
        assert!(artwork_type(b"<html>403</html>").is_err());
        assert!(artwork_type(b"\x89PNG\r\n\x1a\n").is_err());
        assert_eq!(artwork_type(&png()).unwrap(), ("png", "image/png"));
    }
}
