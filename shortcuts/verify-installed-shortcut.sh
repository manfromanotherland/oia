#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
candidate="${1:-$script_dir/Óia Capture.unsigned.shortcut}"
metadata_script="$script_dir/installed-shortcut-metadata.applescript"
runner_script="$script_dir/run-installed-shortcut.applescript"
shortcut_name="${2:-Óia Capture}"

if [[ $# -gt 2 ]]; then
  printf 'Usage: %s [unsigned-shortcut] [shortcut-name]\n' "$0" >&2
  exit 2
fi
if [[ ! -f "$candidate" ]]; then
  printf 'Shortcut candidate not found: %s\n' "$candidate" >&2
  exit 1
fi

for required_command in osascript plutil; do
  command -v "$required_command" >/dev/null 2>&1 || {
    printf 'Installed Shortcut verification requires `%s`.\n' "$required_command" >&2
    exit 1
  }
done

expected_count="$(plutil -extract WFWorkflowActions raw -o - "$candidate")"
probe_version="$(plutil -extract OiaShortcutProbeVersion raw -o - "$candidate")"
expected_fingerprint="$(plutil -extract OiaShortcutFingerprint raw -o - "$candidate")"
if [[ "$probe_version" != "2" || ! "$expected_count" =~ ^[0-9]+$ \
    || ! "$expected_fingerprint" =~ ^[0-9a-f]{64}$ ]]; then
  printf 'Shortcut candidate has invalid release metadata.\n' >&2
  exit 1
fi
case "$expected_count" in
  140|180|220|224|225)
    printf 'Probe-enabled candidate reuses a known unsafe action count: %s.\n' "$expected_count" >&2
    exit 1
    ;;
esac

metadata="$(osascript "$metadata_script" "$shortcut_name")"
IFS=$'\t' read -r match_count installed_id installed_count accepts_input <<< "$metadata"
if [[ "$match_count" != "1" ]]; then
  printf 'Expected one installed %s Shortcut; found %s.\n' "$shortcut_name" "$match_count" >&2
  exit 1
fi
if [[ "$shortcut_name" == "Óia!" && "$accepts_input" != "true" ]]; then
  printf 'Installed %s no longer accepts Share Sheet input.\n' "$shortcut_name" >&2
  exit 1
fi
case "$installed_count" in
  140|180|220|224|225)
    printf 'Installed %s is a known unsafe release (%s actions).\n' \
      "$shortcut_name" "$installed_count" >&2
    printf 'The version probe was not run, so the old Shortcut could not enter capture flow.\n' >&2
    exit 1
    ;;
esac
if [[ "$installed_count" != "$expected_count" ]]; then
  printf 'Installed %s is stale: %s actions installed, %s expected.\n' \
    "$shortcut_name" "$installed_count" "$expected_count" >&2
  printf 'The version probe was not run, so the old Shortcut could not enter capture flow.\n' >&2
  exit 1
fi

expected_output="oia-shortcut/v2 sha256=$expected_fingerprint"
if ! actual_output="$(osascript "$runner_script" "$shortcut_name")"; then
  printf 'Installed %s failed its side-effect-free version probe.\n' "$shortcut_name" >&2
  exit 1
fi
if [[ "$actual_output" != "$expected_output" ]]; then
  printf 'Installed %s release marker does not match the signed candidate.\n' "$shortcut_name" >&2
  exit 1
fi

printf 'Verified installed %s (%s): %s actions, official release marker %s.\n' \
  "$shortcut_name" "$installed_id" "$installed_count" "$expected_fingerprint"
