// SPDX-License-Identifier: MIT

use std::path::PathBuf;

use oia_core::{instagram, LibraryRoot, SaveOutcome, SaveUrlError, UrlSaveRequest};

/// The browser host shares the app's per-device Instagram runtime and bundled
/// script. Downloader paths never come from an extension message or library file.
pub(crate) fn resolve(
    library: &LibraryRoot,
    request: &UrlSaveRequest,
) -> Result<Option<SaveOutcome>, SaveUrlError> {
    resolve_with_instagram(library, request, &|instagram| {
        let home =
            std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("Missing home directory."))?;
        let python =
            PathBuf::from(home).join("Library/Application Support/Oia/Instagram/bin/python3");
        let bundled = std::env::current_exe().ok().and_then(|exe| {
            let macos = exe.parent()?;
            (macos.file_name()? == "MacOS")
                .then(|| {
                    macos
                        .parent()
                        .map(|contents| contents.join("Resources/instagram-download.py"))
                })
                .flatten()
        });
        let script = bundled.unwrap_or_else(|| {
            PathBuf::from("/Applications/Óia.app/Contents/Resources/instagram-download.py")
        });
        instagram::download(&python, &script, instagram)
    })
}

pub(crate) fn resolve_with_instagram(
    library: &LibraryRoot,
    request: &UrlSaveRequest,
    resolver: &instagram::Resolver<'_>,
) -> Result<Option<SaveOutcome>, SaveUrlError> {
    if instagram::is_post_url(&request.url) {
        return instagram::save_with_resolver(library, request, resolver);
    }
    oia_core::save_special_url(library, request)
}
