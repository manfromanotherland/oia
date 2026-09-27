#!/usr/bin/env bash
# Component task runner for the Óia monorepo.
#
# Drive the core, extension, and macOS format / lint / test / build commands
# from the repository root. Git commands run once for the whole monorepo.
#
# Usage:
#   scripts/components.sh <command> [component ...]
#   scripts/components.sh each <shell command ...>
#
# Commands:
#   status   git status -sb for the monorepo
#   push     git push the current monorepo branch (sets upstream)
#   pull     git pull --ff-only for the monorepo
#   fetch    git fetch --all --prune for the monorepo
#   fmt      auto-format every component
#   lint     lint every component
#   test     run unit tests in every component
#   build    build every component
#   check    lint + test          (fast pre-push gate)
#   ci       lint + test + build  (full)
#   each     run any shell command in every component
#   help     this message
#
# Component commands whose toolchain is missing (e.g. xcodebuild off a Mac) are
# skipped with a note rather than failing, so the same runner works on your Mac
# and in the Linux sandbox. It continues past failures and reports which
# components failed, exiting non-zero if any did.
#
# Tip: the root Makefile wraps this — `make push`, `make lint`, `make test`, and
# `make COMPONENT=core test`.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Non-interactive macOS shells may omit rustup's shims even when Rust is
# installed. Add the standard user/Homebrew locations before deciding to skip
# the core checks.
if ! command -v cargo >/dev/null 2>&1; then
  for tool_dir in "$HOME/.cargo/bin" /opt/homebrew/opt/rustup/bin; do
    if [ -x "$tool_dir/cargo" ]; then
      export PATH="$tool_dir:$PATH"
      break
    fi
  done
fi

# Components in quality-check order. Git itself always runs once at the root.
ALL_COMPONENTS="core extension macos"

FAIL=0

# ---- pretty output ---------------------------------------------------------
if [ -t 1 ]; then
  B=$'\033[1m'; DIM=$'\033[2m'; RED=$'\033[31m'; GRN=$'\033[32m'
  YLW=$'\033[33m'; CYN=$'\033[36m'; RST=$'\033[0m'
else
  B=; DIM=; RED=; GRN=; YLW=; CYN=; RST=
fi

have()   { command -v "$1" >/dev/null 2>&1; }
head_branch() { git symbolic-ref --short -q HEAD 2>/dev/null || echo "(detached)"; }

# ---- per-component command mapping -----------------------------------------
# phase_cmd <component> <phase> -> the shell command to run, or empty if it
# does not participate in that phase.
phase_cmd() {
  case "$1:$2" in
    core:fmt)        echo 'cargo fmt --all' ;;
    core:lint)       echo 'cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings' ;;
    core:test)       echo 'cargo test' ;;
    core:build)      echo 'cargo build' ;;

    extension:fmt)   echo 'npm run lint:fix' ;;
    extension:lint)  echo 'npm run lint' ;;
    extension:test)  echo 'npm test' ;;
    extension:build) echo 'npm run build' ;;

    macos:fmt)       echo 'make format' ;;
    macos:lint)      echo 'make format-check && make lint' ;;
    macos:test)      echo 'make test' ;;
    macos:build)     echo 'make all && xcodebuild build -project Oia.xcodeproj -scheme Oia -configuration Debug -derivedDataPath build' ;;

    *)               echo '' ;;
  esac
}

# phase_tool <component> <phase> -> a binary that must be on PATH for the command to
# run; if it is missing the phase is skipped (not failed) for that component.
phase_tool() {
  case "$1" in
    core)              echo cargo ;;
    extension)         echo npm ;;
    macos) case "$2" in fmt) echo swiftformat ;; lint) echo swiftlint ;; *) echo xcodebuild ;; esac ;;
    *)                 echo '' ;;
  esac
}

# ---- runners ---------------------------------------------------------------
run_phase() {
  local phase="$1"; shift
  local components="$*"; [ -z "$components" ] && components="$ALL_COMPONENTS"
  local failed=""
  for component in $components; do
    local cmd; cmd="$(phase_cmd "$component" "$phase")"
    [ -z "$cmd" ] && continue                  # component not part of this phase
    printf '%s\n' "${B}${CYN}==> ${component} · ${phase}${RST}"
    if [ ! -d "$component" ]; then
      printf '%s\n\n' "${YLW}  skipped — $component is not present here${RST}"; continue
    fi
    local tool; tool="$(phase_tool "$component" "$phase")"
    if [ -n "$tool" ] && ! have "$tool"; then
      printf '%s\n\n' "${YLW}  skipped — '$tool' not found in PATH${RST}"; continue
    fi
    printf '%s\n' "${DIM}  \$ ${cmd}${RST}"
    if ( cd "$component" && eval "$cmd" ); then
      printf '%s\n\n' "${GRN}  ✓ ${component} ${phase} ok${RST}"
    else
      failed="$failed $component"
      printf '%s\n\n' "${RED}  ✗ ${component} ${phase} FAILED${RST}"
    fi
  done
  phase_summary "$phase" "$failed"
}

run_git() {
  local phase="$1"; shift
  if [ "$#" -ne 0 ]; then
    printf '%s\n' "${RED}git ${phase} applies to the whole monorepo; component arguments are not supported${RST}"
    FAIL=1
    return
  fi
  local failed=""
  printf '%s\n' "${B}${CYN}==> monorepo · ${phase} ${DIM}($(head_branch))${RST}"
  local rc=0
  case "$phase" in
    status) git status -sb || rc=$? ;;
    push)   git push -u origin HEAD || rc=$? ;;
    pull)   git pull --ff-only || rc=$? ;;
    fetch)  git fetch --all --prune || rc=$? ;;
  esac
  [ "$rc" -ne 0 ] && { failed=" monorepo"; printf '%s\n' "${RED}  ✗ failed${RST}"; }
  printf '\n'
  phase_summary "$phase" "$failed"
}

run_each() {
  local cmd="$*"
  [ -z "$cmd" ] && { echo "usage: $0 each <shell command>"; exit 2; }
  local failed=""
  for component in $ALL_COMPONENTS; do
    [ -d "$component" ] || continue
    printf '%s\n' "${B}${CYN}==> ${component}${RST}"
    printf '%s\n' "${DIM}  \$ ${cmd}${RST}"
    ( cd "$component" && eval "$cmd" ) || failed="$failed $component"
    printf '\n'
  done
  phase_summary "each" "$failed"
}

phase_summary() {
  local phase="$1" failed="$2"
  if [ -n "$failed" ]; then
    printf '%s\n' "${B}${RED}✗ ${phase}: failed in:${failed}${RST}"
    FAIL=1
  else
    printf '%s\n' "${B}${GRN}✓ ${phase}: all good${RST}"
  fi
}

usage() {
  cat <<EOF
${B}Óia monorepo runner${RST}  —  scripts/components.sh <command> [component ...]

  ${B}git${RST}      status  push  pull  fetch        (whole monorepo, one branch)
  ${B}quality${RST}  fmt  lint  test  build            (per-component toolchain)
  ${B}combos${RST}   check (lint+test)   ci (lint+test+build)
  ${B}escape${RST}   each <shell command...>           run in every component

Components: core  extension  macos
Pass component names after a quality command to narrow it:
${DIM}scripts/components.sh lint core extension${RST}
Git commands always operate once on the complete monorepo.

Per-component mapping:
  core       cargo fmt/clippy · cargo test · cargo build
  extension  npm run lint · npm test · npm run build
  macos      make format-check+lint · make test · xcodebuild        (needs a Mac)
Missing toolchains are skipped, not failed — so this works on your Mac and in
the Linux sandbox (where macos build/test are unavailable).
EOF
}

# ---- dispatch --------------------------------------------------------------
cmd="${1:-help}"; shift || true
case "$cmd" in
  status|push|pull|fetch) run_git "$cmd" "$@" ;;
  fmt|format)             run_phase fmt "$@" ;;
  lint)                   run_phase lint "$@" ;;
  test)                   run_phase test "$@" ;;
  build)                  run_phase build "$@" ;;
  check)                  run_phase lint "$@"; run_phase test "$@" ;;
  ci)                     run_phase lint "$@"; run_phase test "$@"; run_phase build "$@" ;;
  each)                   run_each "$@" ;;
  help|-h|--help)         usage ;;
  *)                      echo "unknown command: $cmd"; echo; usage; exit 2 ;;
esac

exit $FAIL
