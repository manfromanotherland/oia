// SPDX-License-Identifier: MIT

//! File-backed photograph classification; the database only projects current results.

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::{
    locking::lock_reading, writer::write_reading_under_lock, ImageClassification, LibraryRoot,
    Metadata, ReadingKind,
};

/// Unknown, stale, or newer classifications never silently become Other images.
pub fn effective_image_type<'a>(
    metadata: &'a Metadata,
    visual_hash: Option<&str>,
) -> Option<&'a str> {
    if metadata.kind != ReadingKind::Image {
        return None;
    }
    let classification = metadata.image_classification.as_ref()?;
    if classification.version != 1
        || !matches!(classification.image_type.as_str(), "photograph" | "other")
    {
        return None;
    }
    match classification.origin.as_str() {
        "user" => Some(classification.image_type.as_str()),
        "machine"
            if visual_hash.is_some()
                && classification.source_fingerprint.as_deref() == visual_hash =>
        {
            Some(classification.image_type.as_str())
        }
        _ => None,
    }
}

/// Correct an image's classification, or clear the override for fresh automatic analysis.
pub fn set_image_type(
    library: &LibraryRoot,
    conn: &Connection,
    id: &str,
    image_type: Option<&str>,
) -> Result<()> {
    let scanned = set_image_type_file(library, id, image_type)?;
    crate::apply_diffs(conn, &[crate::ScanDiff::Changed(scanned)])
}

/// File I/O is separate so FFI callers never hold the DB during image hashing.
pub(crate) fn set_image_type_file(
    library: &LibraryRoot,
    id: &str,
    image_type: Option<&str>,
) -> Result<crate::ScannedReading> {
    if image_type.is_some_and(|value| !matches!(value, "photograph" | "other")) {
        bail!("image type must be photograph or other");
    }
    let lock = lock_reading(library, id)?;
    let Some(mut reading) = crate::scanner::read_reading_for_id(library, id)? else {
        bail!("reading not found: {id}");
    };
    if reading.metadata.kind != ReadingKind::Image {
        bail!("only image readings have an image type");
    }
    reading.metadata.image_classification = image_type.map(|value| ImageClassification {
        version: 1,
        image_type: value.into(),
        origin: "user".into(),
        source_fingerprint: None,
        analyzer_version: None,
    });
    write_reading_under_lock(library, reading.metadata, reading.body, &lock)?;
    crate::tags::scan_for_index(library, id)
}

/// Persist a model result only while it describes the current safe preview bytes.
pub fn complete_image_classification(
    library: &LibraryRoot,
    conn: &Connection,
    id: &str,
    content_hash: &str,
    analyzer_version: &str,
    image_type: &str,
) -> Result<bool> {
    let Some(scanned) = complete_image_classification_file(
        library,
        id,
        content_hash,
        analyzer_version,
        image_type,
    )?
    else {
        return Ok(false);
    };
    crate::apply_diffs(conn, &[crate::ScanDiff::Changed(scanned)])?;
    Ok(true)
}

pub(crate) fn complete_image_classification_file(
    library: &LibraryRoot,
    id: &str,
    content_hash: &str,
    analyzer_version: &str,
    image_type: &str,
) -> Result<Option<crate::ScannedReading>> {
    if analyzer_version.trim().is_empty() {
        bail!("analyzer version must not be blank");
    }
    if !matches!(image_type, "photograph" | "other" | "unknown") {
        bail!("image type must be photograph, other, or unknown");
    }
    let lock = lock_reading(library, id)?;
    let Some(reading) = crate::scanner::read_reading_for_id(library, id)? else {
        return Ok(None);
    };
    if !accepts_machine_result(&reading.metadata) {
        return Ok(None);
    }
    let Some(path) = reading.metadata.preview_asset.as_deref() else {
        return Ok(None);
    };
    let Ok(asset) = crate::visual_index::inspect_image_asset_for_tagging(library, id, path) else {
        return Ok(None);
    };
    if asset.content_hash != content_hash {
        return Ok(None);
    }
    // External writers do not participate in the process lock. Re-read after
    // hashing to retain a correction or unrelated file edit made during I/O.
    let Some(mut latest) = crate::scanner::read_reading_for_id(library, id)? else {
        return Ok(None);
    };
    if !accepts_machine_result(&latest.metadata)
        || latest.metadata.preview_asset.as_deref() != Some(path)
    {
        return Ok(None);
    }
    let result = ImageClassification {
        version: 1,
        image_type: image_type.into(),
        origin: "machine".into(),
        source_fingerprint: Some(content_hash.into()),
        analyzer_version: Some(analyzer_version.into()),
    };
    if latest.metadata.image_classification.as_ref() != Some(&result) {
        latest.metadata.image_classification = Some(result);
        write_reading_under_lock(library, latest.metadata, latest.body, &lock)?;
    }
    Ok(Some(crate::tags::scan_for_index(library, id)?))
}

fn accepts_machine_result(metadata: &Metadata) -> bool {
    metadata.kind == ReadingKind::Image
        && metadata
            .image_classification
            .as_ref()
            .is_none_or(|current| {
                current.version == 1
                    && current.origin == "machine"
                    && matches!(
                        current.image_type.as_str(),
                        "photograph" | "other" | "unknown"
                    )
            })
}

/// Filter a whole bounded candidate window before staging. Cursor advancement
/// belongs to the caller's original window, including already classified rows.
pub(crate) fn pending_candidates(
    library: &LibraryRoot,
    analyzer_version: &str,
    candidates: Vec<crate::visual_index::VisualAssetCandidate>,
) -> Result<Vec<crate::visual_index::VisualAssetCandidate>> {
    if analyzer_version.trim().is_empty() {
        bail!("analyzer version must not be blank");
    }
    Ok(candidates
        .into_iter()
        .filter(|(id, _, path, hash)| {
            let Ok(Some(reading)) = crate::scanner::read_reading_for_id(library, id) else {
                return false;
            };
            accepts_machine_result(&reading.metadata)
                && reading.metadata.preview_asset.as_deref() == Some(path.as_str())
                && !reading
                    .metadata
                    .image_classification
                    .as_ref()
                    .is_some_and(|current| {
                        current.source_fingerprint.as_deref() == Some(hash.as_str())
                            && current.analyzer_version.as_deref() == Some(analyzer_version)
                    })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{get_reading, import_image, list_readings, rebuild, CountScope, ListOptions};
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, TempDir, LibraryRoot, Connection) {
        let files = TempDir::new().unwrap();
        fs::create_dir(files.path().join("articles")).unwrap();
        let library = LibraryRoot::new(files.path()).unwrap();
        let cache = TempDir::new().unwrap();
        let conn = crate::open_index(&cache.path().join("index.db")).unwrap();
        (files, cache, library, conn)
    }

    fn png(width: u32) -> Vec<u8> {
        let mut bytes = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&400_u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn save(library: &LibraryRoot, width: u32) -> (String, String) {
        let bytes = png(width);
        let hash = crate::sha256_hex(&bytes);
        let saved = import_image(library, bytes, "image/png", "Garden design").unwrap();
        (saved.id, hash)
    }

    fn current_type(conn: &Connection, id: &str) -> Option<String> {
        get_reading(conn, id).unwrap().unwrap().0.image_type
    }

    #[test]
    fn manual_correction_survives_machine_result_duplicate_save_and_rebuild() {
        let (_files, _cache, library, conn) = setup();
        let (id, hash) = save(&library, 400);
        rebuild(&conn, &library).unwrap();
        assert!(complete_image_classification(
            &library,
            &conn,
            &id,
            &hash,
            "model-v1",
            "photograph"
        )
        .unwrap());
        assert_eq!(current_type(&conn, &id).as_deref(), Some("photograph"));
        set_image_type(&library, &conn, &id, Some("other")).unwrap();
        assert!(!complete_image_classification(
            &library,
            &conn,
            &id,
            &hash,
            "model-v2",
            "photograph"
        )
        .unwrap());
        let duplicate = import_image(&library, png(400), "image/png", "New title").unwrap();
        assert_eq!(duplicate.disposition, crate::SaveDisposition::Duplicate);
        rebuild(&conn, &library).unwrap();
        assert_eq!(current_type(&conn, &id).as_deref(), Some("other"));
        let file = crate::read_metadata(&library.article_path(&id)).unwrap();
        assert_eq!(file.image_classification.unwrap().origin, "user");
        set_image_type(&library, &conn, &id, None).unwrap();
        assert_eq!(current_type(&conn, &id), None);
        assert!(complete_image_classification(
            &library,
            &conn,
            &id,
            &hash,
            "model-v2",
            "photograph"
        )
        .unwrap());
        assert_eq!(current_type(&conn, &id).as_deref(), Some("photograph"));
        assert!(set_image_type(&library, &conn, &id, Some("future-type")).is_err());
    }

    #[test]
    fn replaced_preview_invalidates_machine_classification_and_rejects_old_completion() {
        let (_files, _cache, library, conn) = setup();
        let (id, hash) = save(&library, 410);
        rebuild(&conn, &library).unwrap();
        complete_image_classification(&library, &conn, &id, &hash, "model-v1", "photograph")
            .unwrap();
        let metadata = crate::read_metadata(&library.article_path(&id)).unwrap();
        fs::write(
            library
                .reading_dir(&id)
                .join(metadata.preview_asset.unwrap()),
            png(420),
        )
        .unwrap();
        rebuild(&conn, &library).unwrap();
        assert_eq!(current_type(&conn, &id), None);
        assert!(
            !complete_image_classification(&library, &conn, &id, &hash, "model-v1", "other")
                .unwrap()
        );
        set_image_type(&library, &conn, &id, Some("photograph")).unwrap();
        rebuild(&conn, &library).unwrap();
        assert_eq!(current_type(&conn, &id).as_deref(), Some("photograph"));
    }

    #[test]
    fn photo_other_unknown_filters_match_search_and_counts_after_rebuild() {
        let (_files, _cache, library, conn) = setup();
        let (photo, photo_hash) = save(&library, 430);
        let (other, other_hash) = save(&library, 440);
        let (unknown, unknown_hash) = save(&library, 450);
        rebuild(&conn, &library).unwrap();
        for (id, hash, value) in [
            (&photo, &photo_hash, "photograph"),
            (&other, &other_hash, "other"),
            (&unknown, &unknown_hash, "unknown"),
        ] {
            complete_image_classification(&library, &conn, id, hash, "model-v1", value).unwrap();
        }
        rebuild(&conn, &library).unwrap();
        for (term, expected) in [
            ("photograph", &photo),
            ("other_image", &other),
            ("unclassified_image", &unknown),
        ] {
            for query in [
                None,
                Some("Garden design".into()),
                Some("design Garden".into()),
            ] {
                let rows = list_readings(
                    &conn,
                    &ListOptions {
                        query: query.clone(),
                        item_type_terms: vec!["image".into(), term.into()],
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(
                    rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
                    vec![expected]
                );
                let counts = crate::view_counts(
                    &conn,
                    &CountScope {
                        query,
                        item_type_terms: vec!["image".into(), term.into()],
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(counts.all, 1);
            }
        }
        assert_eq!(
            list_readings(
                &conn,
                &ListOptions {
                    item_type_terms: vec!["photograph".into(), "other_image".into()],
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            0
        );
    }

    #[test]
    fn unknown_is_a_processed_result_until_bytes_or_analyzer_change() {
        let (_files, _cache, library, conn) = setup();
        let (id, hash) = save(&library, 460);
        rebuild(&conn, &library).unwrap();
        let candidates =
            || crate::visual_index::visual_asset_candidate_batch(&conn, None, 64).unwrap();
        assert_eq!(
            pending_candidates(&library, "model-v1", candidates())
                .unwrap()
                .len(),
            1
        );
        complete_image_classification(&library, &conn, &id, &hash, "model-v1", "unknown").unwrap();
        assert!(pending_candidates(&library, "model-v1", candidates())
            .unwrap()
            .is_empty());
        assert_eq!(
            pending_candidates(&library, "model-v2", candidates())
                .unwrap()
                .len(),
            1
        );
        set_image_type(&library, &conn, &id, Some("other")).unwrap();
        assert!(pending_candidates(&library, "model-v2", candidates())
            .unwrap()
            .is_empty());
        set_image_type(&library, &conn, &id, None).unwrap();
        assert_eq!(
            pending_candidates(&library, "model-v2", candidates())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn pending_batch_advances_across_a_window_of_processed_results() {
        let (_files, cache, library, conn) = setup();
        let mut ids: Vec<_> = [480, 490, 500]
            .into_iter()
            .map(|width| save(&library, width).0)
            .collect();
        ids.sort();
        rebuild(&conn, &library).unwrap();
        for id in &ids[..2] {
            set_image_type(&library, &conn, id, Some("photograph")).unwrap();
        }
        let database =
            crate::ffi::Database::open(cache.path().join("index.db").display().to_string())
                .unwrap();
        let path = library.path().display().to_string();
        let first = database
            .pending_image_classification_batch(path.clone(), "model-v1".into(), 2, None)
            .unwrap();
        assert!(first.assets.is_empty());
        assert_eq!(first.next_reading_id.as_deref(), Some(ids[1].as_str()));
        let last = database
            .pending_image_classification_batch(path, "model-v1".into(), 2, first.next_reading_id)
            .unwrap();
        assert_eq!(last.assets.len(), 1);
        assert_eq!(last.assets[0].reading_id, ids[2]);
        assert!(last.next_reading_id.is_none());
    }

    #[test]
    fn future_classification_values_survive_roundtrip_without_matching_other_images() {
        let (_files, _cache, library, conn) = setup();
        let (id, hash) = save(&library, 470);
        let mut reading =
            crate::parse_reading(&fs::read_to_string(library.article_path(&id)).unwrap()).unwrap();
        for (version, image_type) in [(1, "future-type"), (2, "photograph")] {
            let classification = ImageClassification {
                version,
                image_type: image_type.into(),
                origin: "machine".into(),
                source_fingerprint: Some(hash.clone()),
                analyzer_version: Some("model-v1".into()),
            };
            reading.metadata.image_classification = Some(classification.clone());
            crate::write_reading(&library, reading.metadata.clone(), reading.body.clone()).unwrap();
            let parsed = crate::read_metadata(&library.article_path(&id)).unwrap();
            assert_eq!(parsed.image_classification, Some(classification));
            rebuild(&conn, &library).unwrap();
            assert_eq!(current_type(&conn, &id), None);
            assert!(!complete_image_classification(
                &library, &conn, &id, &hash, "model-v2", "other"
            )
            .unwrap());
            assert!(list_readings(
                &conn,
                &ListOptions {
                    item_type_terms: vec!["other_image".into()],
                    ..Default::default()
                }
            )
            .unwrap()
            .is_empty());
        }
    }
}
