# Reviewed Tag renames

`oia-sanitize-tags` applies an explicit rename map to existing user Tags, each machine Tag
source, and machine Tag exclusions. It consolidates matching labels using the core's
case-insensitive Tag identity. User ownership and machine provenance remain intact.

Prepare a JSON object containing only the names to change:

```json
{
  "motion-design": "motion design",
  "visual-effects": "visual effects"
}
```

Names omitted from the map retain their spelling, so compounds such as `t-shirt` and
`well-being` can remain unchanged. Replacements must be nonempty, trimmed, and no longer
than their original label in Unicode scalar values. Long legacy labels are supported.
Conflicting case-insensitive entries and rename chains are rejected.

Preview from the repository root:

```sh
cargo run --manifest-path core/Cargo.toml -p oia-import-mymind --bin oia-sanitize-tags -- \
  --library "/path/to/library" --map "/path/to/renames.json" > /tmp/tag-preview.json
```

Review `planned`, `renamed_labels`, `merged_duplicates`, and `renames` in the JSON report.
The preview reads files without changing the library. Its `digest` covers the rename map
and the complete article-file inventory, including each file's content hash.

Apply using that digest and an empty backup folder outside the library:

```sh
cargo run --manifest-path core/Cargo.toml -p oia-import-mymind --bin oia-sanitize-tags -- \
  --library "/path/to/library" --map "/path/to/renames.json" \
  --apply --expect-digest "DIGEST_FROM_PREVIEW" --backup-dir "/path/to/empty-backup"
```

A changed map or inventory requires a fresh preview. Each write holds the existing reading
lock, checks its file hash and directory identity again immediately before an atomic commit,
and leaves concurrent edits or deleted readings untouched. Conflicts are listed in the report
and produce a nonzero exit status.

The backup contains an original `<reading-id>.md` for each changed card, a durable
`journal.jsonl`, and `manifest.json` with the original and changed hashes. Only Tag fields are
reserialized; unrelated frontmatter and raw Markdown body bytes remain intact. Assets and
sidecars are untouched. The app's library watcher reconciles the disposable index.
