#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sandbox="$(mktemp -d "${TMPDIR:-/tmp}/wtherdr-publishing.XXXXXX")"
trap 'rm -rf "$sandbox"' EXIT
mkdir -p "$sandbox/bin" "$sandbox/mise-tasks/version"
printf '#!/bin/sh\nprintf "0.0.1\\n"\n' >"$sandbox/mise-tasks/version/read"
printf '#!/bin/sh\nexit 0\n' >"$sandbox/mise-tasks/version/check"
cat >"$sandbox/bin/git" <<'SCRIPT'
#!/bin/sh
case "$1" in
  status) printf '%s' "${TEST_DIRTY:-}" ;;
  rev-parse)
    if [ "$2" = HEAD ]; then printf 'abc\n'; else printf '%s\n' "${TEST_TAG_HEAD:-abc}"; fi ;;
  *) exit 1 ;;
esac
SCRIPT
cat >"$sandbox/bin/gh" <<'SCRIPT'
#!/bin/sh
if [ "${TEST_RELEASE_FAIL:-0}" = 1 ]; then exit 1; fi
if [ "${TEST_DRAFT:-0}" = 1 ]; then printf 'true\n'; exit; fi
case "$*" in
  *isDraft*) printf 'false\n' ;;
  *)
    for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin; do
      printf 'wtherdr-%s-v0.0.1.tgz\n' "$target"
      if [ "${TEST_MISSING_CHECKSUM:-0}" != 1 ]; then
        printf 'wtherdr-%s-v0.0.1.tgz.sha256\n' "$target"
      fi
    done ;;
esac
SCRIPT
chmod +x "$sandbox/bin/"* "$sandbox/mise-tasks/version/"*
run_check() {
  MISE_PROJECT_ROOT="$sandbox" PATH="$sandbox/bin:$PATH" bash "$repo_root/mise-tasks/release/crate-preflight"
}
expect_failure() {
  if run_check >"$sandbox/output" 2>&1; then
    printf 'Publication guard unexpectedly accepted %s\n' "$1" >&2
    exit 1
  fi
  grep -Fq "$2" "$sandbox/output"
}
run_check
export TEST_DIRTY=' M Cargo.toml'
expect_failure 'dirty source' 'clean checkout'
unset TEST_DIRTY
export TEST_TAG_HEAD=def
expect_failure 'wrong tag' 'does not match'
unset TEST_TAG_HEAD
export TEST_MISSING_CHECKSUM=1
expect_failure 'missing checksums' 'Missing release asset'
unset TEST_MISSING_CHECKSUM
export TEST_DRAFT=1
expect_failure 'draft release' 'published release'
unset TEST_DRAFT
export TEST_RELEASE_FAIL=1
expect_failure 'unavailable release' 'GitHub release'
if GITHUB_ACTIONS=false GITHUB_EVENT_NAME=workflow_dispatch CARGO_REGISTRY_TOKEN='' bash "$repo_root/mise-tasks/release/publish-crate" >"$sandbox/output" 2>&1; then
  printf 'OIDC publishing accepted a non-release invocation.\n' >&2
  exit 1
fi
grep -Fq 'requires a GitHub release job' "$sandbox/output"
printf 'Crate publication guard tests passed.\n'
