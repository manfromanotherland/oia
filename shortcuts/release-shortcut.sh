#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: %s [--install]\n' "$0" >&2
  exit 2
}

install=false
if [[ $# -gt 1 ]]; then
  usage
fi

case "${1:-}" in
  "") ;;
  --install) install=true ;;
  *) usage ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
unsigned="$script_dir/Óia Capture.unsigned.shortcut"
signed="$script_dir/Óia Capture.shortcut"
shortcut_name="Óia Capture"
module_cache="${TMPDIR:-/tmp}/oia-shortcut-module-cache"
release_dir=""
unsigned_publish=""
signed_publish=""
unsigned_restore=""
signed_restore=""
rollback_publish=false

cleanup() {
  cleanup_status=$?
  set +e
  if [[ "$rollback_publish" == true ]]; then
    unsigned_restore="$script_dir/.Óia Capture.unsigned.shortcut.restore.$$"
    signed_restore="$script_dir/.Óia Capture.shortcut.restore.$$"
    if [[ -f "$release_dir/previous.unsigned.shortcut" ]]; then
      cp "$release_dir/previous.unsigned.shortcut" "$unsigned_restore"
      chmod 0644 "$unsigned_restore"
      mv "$unsigned_restore" "$unsigned"
    else
      rm -f "$unsigned"
    fi
    if [[ -f "$release_dir/previous.shortcut" ]]; then
      cp "$release_dir/previous.shortcut" "$signed_restore"
      chmod 0600 "$signed_restore"
      mv "$signed_restore" "$signed"
    else
      rm -f "$signed"
    fi
    rollback_publish=false
  fi
  if [[ -n "$release_dir" && -d "$release_dir" ]]; then
    rm -rf "$release_dir"
  fi
  if [[ -n "$unsigned_publish" ]]; then
    rm -f "$unsigned_publish"
  fi
  if [[ -n "$signed_publish" ]]; then
    rm -f "$signed_publish"
  fi
  if [[ -n "$unsigned_restore" ]]; then
    rm -f "$unsigned_restore"
  fi
  if [[ -n "$signed_restore" ]]; then
    rm -f "$signed_restore"
  fi
  if [[ "$install" == true && -n "${launcher_before:-}" ]]; then
    launcher_after="$(osascript "$script_dir/installed-shortcut-metadata.applescript" 'Óia!' 2>/dev/null)"
    if [[ "$launcher_after" != "$launcher_before" ]]; then
      printf 'Permanent Óia! metadata changed during helper installation. Inspect before continuing.\n' >&2
      exit 1
    fi
  fi
  return "$cleanup_status"
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for required_command in swift shortcuts shasum mktemp cp chmod mv; do
  command -v "$required_command" >/dev/null 2>&1 || {
    printf 'Shortcut release requires `%s`.\n' "$required_command" >&2
    exit 1
  }
done

if [[ "$install" == true ]]; then
  command -v open >/dev/null 2>&1 || {
    printf 'Shortcut installation requires macOS and the `open` command.\n' >&2
    exit 1
  }
  if [[ ! -x "$script_dir/verify-installed-shortcut.sh" ]]; then
    printf 'Installed Shortcut verifier is missing or not executable.\n' >&2
    exit 1
  fi
fi

launcher_before=""
if [[ "$install" == true ]]; then
  launcher_before="$(osascript "$script_dir/installed-shortcut-metadata.applescript" 'Óia!')"
fi
mkdir -p "$module_cache"
cd "$repo_root"

export CLANG_MODULE_CACHE_PATH="$module_cache"
export SWIFT_MODULECACHE_PATH="$module_cache"

release_dir="$(mktemp -d "${TMPDIR:-/tmp}/oia-shortcut-release.XXXXXX")"
candidate_unsigned="$release_dir/Óia Capture.unsigned.shortcut"
candidate_signed="$release_dir/Óia Capture.shortcut"
source_fingerprint="$(shasum -a 256 shortcuts/build-shortcut.swift)"
source_fingerprint="${source_fingerprint%% *}"

swift shortcuts/build-shortcut.swift "$candidate_unsigned" "$source_fingerprint"
swift shortcuts/test-shortcut.swift "$candidate_unsigned"
swift shortcuts/validate-shortcut.swift "$candidate_unsigned"
swift shortcuts/conflicting-shortcut-names.swift --test
swift shortcuts/test-launcher.swift 'shortcuts/Óia!.unsigned.shortcut'
swift shortcuts/validate-shortcut.swift 'shortcuts/Óia!.unsigned.shortcut'
shortcuts sign --mode anyone --input "$candidate_unsigned" --output "$candidate_signed"

unsigned_publish="$script_dir/.Óia Capture.unsigned.shortcut.publish.$$"
signed_publish="$script_dir/.Óia Capture.shortcut.publish.$$"
[[ ! -f "$unsigned" ]] || cp "$unsigned" "$release_dir/previous.unsigned.shortcut"
[[ ! -f "$signed" ]] || cp "$signed" "$release_dir/previous.shortcut"
cp "$candidate_unsigned" "$unsigned_publish"
cp "$candidate_signed" "$signed_publish"
chmod 0644 "$unsigned_publish"
chmod 0600 "$signed_publish"
rollback_publish=true
mv "$unsigned_publish" "$unsigned"
unsigned_publish=""
mv "$signed_publish" "$signed"
signed_publish=""
rollback_publish=false

printf '\nSigned Shortcut: %s\n' "$signed"

if [[ "$install" == true ]]; then
  if verification="$(shortcuts/verify-installed-shortcut.sh "$unsigned" "$shortcut_name" 2>/dev/null)"; then
    printf '%s\n' "$verification"
    exit 0
  fi

  # Capture errors instead of treating a failed listing as an empty library.
  # Apple's displayed names can use a decomposed accent; normalize for matching.
  installed_names="$(shortcuts list)"
  conflicting_shortcuts="$(printf '%s\n' "$installed_names" |
    swift shortcuts/conflicting-shortcut-names.swift "$shortcut_name")"

  if [[ -n "$conflicting_shortcuts" ]]; then
    printf '%s\n' \
      '' \
      'Refusing to import while the Óia Capture helper is already installed.' \
      'Importing beside it would create another numbered copy.' \
      'Update only Óia Capture in place or replace that helper, then rerun. Preserve the existing Óia! Shortcut.' \
      '' \
      'Installed conflicts:' >&2
    while IFS= read -r installed_name; do
      printf '  - %s\n' "$installed_name" >&2
    done <<< "$conflicting_shortcuts"
    printf 'Signed replacement ready at: %s\n' "$signed" >&2
    exit 1
  fi

  open -g "$signed"
  printf '%s\n' \
    '' \
    'Opened the signed Shortcut import in the background.' \
    'Waiting for the installed copy to return the expected official release marker…'
  attempts=0
  last_failure=""
  while [[ $attempts -lt 90 ]]; do
    if verification="$(shortcuts/verify-installed-shortcut.sh "$unsigned" "$shortcut_name" 2>&1)"; then
      printf '%s\n' "$verification"
      exit 0
    fi
    last_failure="$verification"
    attempts=$((attempts + 1))
    sleep 2
  done
  printf 'Capture helper update was not verified after 180 seconds.\n%s\n' "$last_failure" >&2
  exit 1
else
  printf '%s\n' \
    'The helper artifact is signed but the installed helper has not been verified.' \
    'Run `make shortcut-install` before calling this release complete.'
fi
