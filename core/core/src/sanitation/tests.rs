// SPDX-License-Identifier: MIT

use super::*;
use tempfile::TempDir;

fn fixture() -> (TempDir, LibraryRoot, String, String) {
    let dir = TempDir::new().unwrap();
    let library = LibraryRoot::new(dir.path()).unwrap();
    let saved = crate::import_link(&library, "https://example.com/tag-migration").unwrap();
    let initial = fs::read_to_string(library.article_path(&saved.id)).unwrap();
    let initial = initial.replace(
        "tags: []",
        "tags: [motion-design, 'motion design', well-being, 'Mixed Case']",
    );
    let original = initial.replacen("\n---\n", "\nmachine_tags:\n- source: text\n  source_fingerprint: unchanged-text-fingerprint\n  analyzer_version: text-v1\n  custom_source_key: 'Keep this too'\n  tags: [motion-design, 'motion design', story-telling]\n- source: image\n  source_fingerprint: unchanged-image-fingerprint\n  analyzer_version: image-v2\n  tags: [motion-design, well-being]\nexcluded_machine_tags: [motion-design, 'motion design', real-time, UNRELATED]\n# external comment stays verbatim\nunknown_future_key: 'Keep exactly this!'\n---\n", 1) + "Body with trailing spaces  \n\n\n";
    fs::write(library.article_path(&saved.id), &original).unwrap();
    fs::write(library.note_path(&saved.id), "Private sidecar\n").unwrap();
    fs::write(
        library.assets_dir(&saved.id).join("asset.bin"),
        b"assets unchanged",
    )
    .unwrap();
    (dir, library, saved.id, original)
}

fn names() -> BTreeMap<String, String> {
    BTreeMap::from([("motion-design".into(), "motion design".into())])
}

#[test]
fn explicit_renames_merge_collisions_and_preserve_body_provenance_and_compounds() {
    let (_dir, library, id, original) = fixture();
    let backup = TempDir::new().unwrap();
    let plan = plan_tag_renames(&library, &names()).unwrap();
    assert_eq!(plan.report.scanned, 1);
    assert_eq!(plan.report.planned, 1);
    assert_eq!(plan.report.renamed_labels, 4);
    assert_eq!(plan.report.merged_duplicates, 3);
    assert_eq!(
        fs::read_to_string(library.article_path(&id)).unwrap(),
        original
    );
    let expected = plan.report.digest.clone();
    let report = apply_tag_renames(&library, plan, &expected, backup.path()).unwrap();
    assert_eq!(report.changed, 1);
    assert_eq!(report.conflicts, 0);
    let changed = fs::read_to_string(library.article_path(&id)).unwrap();
    assert_eq!(
        original.split_once("\n---\n").unwrap().1,
        changed.split_once("\n---\n").unwrap().1
    );
    assert!(changed
        .contains("# external comment stays verbatim\nunknown_future_key: 'Keep exactly this!'"));
    assert!(changed.contains("custom_source_key: Keep this too"));
    let before = crate::parse_reading(&original).unwrap();
    let after = crate::parse_reading(&changed).unwrap();
    let mut expected_metadata = before.metadata;
    expected_metadata.tags = vec![
        "motion design".into(),
        "well-being".into(),
        "Mixed Case".into(),
    ];
    expected_metadata.machine_tags[0].tags = vec!["motion design".into(), "story-telling".into()];
    expected_metadata.machine_tags[1].tags = vec!["motion design".into(), "well-being".into()];
    expected_metadata.excluded_machine_tags = vec![
        "motion design".into(),
        "real-time".into(),
        "UNRELATED".into(),
    ];
    assert_eq!(after.metadata, expected_metadata);
    let effective = crate::tags::tag_entries(&after.metadata);
    assert!(effective
        .iter()
        .any(|tag| tag.key == "motion design" && tag.origin == "user"));
    assert_eq!(
        fs::read(backup.path().join(format!("{id}.md"))).unwrap(),
        original.as_bytes()
    );
    assert!(backup.path().join("manifest.json").is_file());
    assert_eq!(
        report.files[&id].original_sha256,
        crate::sha256_hex(original.as_bytes())
    );
    assert_eq!(
        fs::read(library.note_path(&id)).unwrap(),
        b"Private sidecar\n"
    );
    assert_eq!(
        fs::read(library.assets_dir(&id).join("asset.bin")).unwrap(),
        b"assets unchanged"
    );
    assert_eq!(
        plan_tag_renames(&library, &names()).unwrap().report.planned,
        0
    );
}

#[test]
fn snapshot_digest_guards_entire_inventory_and_rename_map_before_writing() {
    let (_dir, library, id, original) = fixture();
    let digest = plan_tag_renames(&library, &names()).unwrap().report.digest;
    let changed_names = BTreeMap::from([("story-telling".into(), "story telling".into())]);
    let changed_map = plan_tag_renames(&library, &changed_names).unwrap();
    let backup = TempDir::new().unwrap();
    assert!(apply_tag_renames(&library, changed_map, &digest, backup.path()).is_err());
    assert!(fs::read_dir(backup.path()).unwrap().next().is_none());
    fs::write(library.article_path(&id), original + "external edit\n").unwrap();
    let changed_file = plan_tag_renames(&library, &names()).unwrap();
    assert!(apply_tag_renames(&library, changed_file, &digest, backup.path()).is_err());
    assert!(fs::read_dir(backup.path()).unwrap().next().is_none());
}

#[test]
fn deleted_readings_are_skipped_without_recreating_their_folder_or_backup() {
    let (_dir, library, id, _) = fixture();
    let plan = plan_tag_renames(&library, &names()).unwrap();
    let digest = plan.report.digest.clone();
    fs::remove_dir_all(library.reading_dir(&id)).unwrap();
    let backup = TempDir::new().unwrap();
    let report = apply_tag_renames(&library, plan, &digest, backup.path()).unwrap();
    assert_eq!(report.changed, 0);
    assert_eq!(report.conflicts, 1);
    assert_eq!(report.conflict_ids, vec![id.clone()]);
    assert!(!library.reading_dir(&id).exists());
    assert!(!backup.path().join(format!("{id}.md")).exists());
}

#[test]
fn external_edits_immediately_before_commit_are_preserved() {
    let (_dir, library, id, original) = fixture();
    let plan = plan_tag_renames(&library, &names()).unwrap();
    let backup = TempDir::new().unwrap();
    let external = format!("{original}Another writer changed this\n");
    let changed = apply_target(&library, &plan.targets[0], backup.path(), || {
        fs::write(library.article_path(&id), &external).unwrap();
    })
    .unwrap();
    assert!(!changed);
    assert_eq!(
        fs::read_to_string(library.article_path(&id)).unwrap(),
        external
    );
    assert!(!backup.path().join(format!("{id}.md")).exists());
    assert!(!fs::read_dir(library.reading_dir(&id))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
}

#[test]
fn replaced_reading_directories_are_not_committed_through_a_pinned_old_handle() {
    let (dir, library, id, original) = fixture();
    let plan = plan_tag_renames(&library, &names()).unwrap();
    let backup = TempDir::new().unwrap();
    let moved = dir.path().join("moved-reading");
    let changed = apply_target(&library, &plan.targets[0], backup.path(), || {
        fs::rename(library.reading_dir(&id), &moved).unwrap();
        fs::create_dir(library.reading_dir(&id)).unwrap();
        fs::write(library.article_path(&id), &original).unwrap();
    })
    .unwrap();
    assert!(!changed);
    assert_eq!(
        fs::read_to_string(library.article_path(&id)).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(moved.join("article.md")).unwrap(),
        original
    );
}

#[test]
fn case_folded_mapping_keys_preserve_nonmatching_spelling_and_long_legacy_labels() {
    let map = BTreeMap::from([
        ("MOTION-DESIGN".into(), "motion design".into()),
        (
            "an-existing-legacy-label-over-twenty".into(),
            "an existing legacy label over twenty".into(),
        ),
    ]);
    let normalized = validate_names(&map).unwrap();
    let mut labels = Value::Sequence(vec![
        Value::String("Motion-Design".into()),
        Value::String("Mixed Case".into()),
        Value::String("an-existing-legacy-label-over-twenty".into()),
    ]);
    rename_list(
        &mut labels,
        &normalized,
        false,
        &mut SanitationReport::default(),
    )
    .unwrap();
    assert_eq!(
        labels,
        Value::Sequence(vec![
            Value::String("motion design".into()),
            Value::String("Mixed Case".into()),
            Value::String("an existing legacy label over twenty".into())
        ])
    );
    assert!(validate_names(&BTreeMap::from([("a-b".into(), "a bee".into())])).is_err());
    assert!(validate_names(&BTreeMap::from([
        ("a-b".into(), "a b".into()),
        ("A-B".into(), "a B".into())
    ]))
    .is_err());
    assert!(validate_names(&BTreeMap::from([
        ("a-b".into(), "a b".into()),
        ("a b".into(), "ab".into())
    ]))
    .is_err());
}

#[test]
fn normal_serialized_unindented_tag_arrays_are_replaced_without_old_rows_remaining() {
    let (_dir, _library, _id, original) = fixture();
    let mut reading = crate::parse_reading(&original).unwrap();
    reading.metadata.tags = vec![
        "motion-design".into(),
        "motion design".into(),
        "prefix: untouched-compound".into(),
    ];
    let serialized = crate::render_reading(&reading).unwrap();
    assert!(serialized.contains("tags:\n- motion-design\n"));
    let patched = patch_tags(&serialized, &names(), &mut SanitationReport::default())
        .unwrap()
        .unwrap();
    let updated = crate::parse_reading(&patched).unwrap();
    assert_eq!(
        updated.metadata.tags,
        vec!["motion design", "prefix: untouched-compound"]
    );
    assert_eq!(
        updated.metadata.machine_tags[0].tags,
        vec!["motion design", "story-telling"]
    );
    assert_eq!(
        updated.body,
        crate::parse_reading(&serialized).unwrap().body
    );
}

#[test]
fn backups_inside_the_library_are_rejected_before_any_change() {
    let (_dir, library, id, original) = fixture();
    let plan = plan_tag_renames(&library, &names()).unwrap();
    let digest = plan.report.digest.clone();
    assert!(apply_tag_renames(&library, plan, &digest, &library.path().join("backup")).is_err());
    assert_eq!(
        fs::read_to_string(library.article_path(&id)).unwrap(),
        original
    );
    assert!(!library.path().join("backup").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_articles_are_rejected_instead_of_followed_outside_the_library() {
    let (dir, library, id, original) = fixture();
    let outside = dir.path().join("outside.md");
    fs::write(&outside, &original).unwrap();
    fs::remove_file(library.article_path(&id)).unwrap();
    std::os::unix::fs::symlink(&outside, library.article_path(&id)).unwrap();
    assert!(plan_tag_renames(&library, &names()).is_err());
    assert_eq!(fs::read_to_string(outside).unwrap(), original);
}
