#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sandbox="$(mktemp -d "${TMPDIR:-/tmp}/wtherdr-versioning.XXXXXX")"
trap 'rm -rf "$sandbox"' EXIT

fail() {
	printf 'FAIL: %s\n' "$1" >&2
	exit 1
}

assert_contains() {
	local output="$1"
	local expected="$2"
	[[ "$output" == *"$expected"* ]] || fail "expected output to contain: $expected"
}

fakebin="$sandbox/bin"
mkdir -p "$fakebin"

cat >"$fakebin/cargo" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != update || "${2:-}" != --workspace ]]; then
  printf 'unexpected cargo command: %s\n' "$*" >&2
  exit 1
fi

name="$(awk '
  $0 == "[package]" { package = 1; next }
  package && /^\[/ { exit }
  package && /^[[:space:]]*name[[:space:]]*=/ { sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit }
' Cargo.toml)"
version="$(awk '
  $0 == "[package]" { package = 1; next }
  package && /^\[/ { exit }
  package && /^[[:space:]]*version[[:space:]]*=/ { sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit }
' Cargo.toml)"
awk -v name="$name" -v version="$version" '
  /^\[\[package\]\]$/ { wtherdr = 0 }
  /^name = "wtherdr"$/ || /^name = "stale-package"$/ {
    print "name = \"" name "\""
    wtherdr = 1
    next
  }
  wtherdr && /^version = / { print "version = \"" version "\""; wtherdr = 0; next }
  { print }
' Cargo.lock >Cargo.lock.fake
mv Cargo.lock.fake Cargo.lock
printf '{}\n'
FAKE
chmod +x "$fakebin/cargo"

make_repo() {
	local name="$1"
	local cargo_version="$2"
	local lock_version="$3"
	local manifest_version="$4"
	local lock_name="${5:-wtherdr}"
	local root="$sandbox/$name"

	mkdir -p "$root"
	cp -R "$repo_root/mise-tasks" "$root/mise-tasks"

	cat >"$root/Cargo.toml" <<TOML
[package]
name = "wtherdr"
version = "$cargo_version"
edition = "2024"

[dependencies]
version = "1.0"
TOML

	cat >"$root/Cargo.lock" <<TOML
version = 4

[[package]]
name = "anyhow"
version = "1.0.99"

[[package]]
name = "$lock_name"
version = "$lock_version"
TOML

	cat >"$root/herdr-plugin.toml" <<TOML
id = "wtherdr"
name = "wtherdr"
version = "$manifest_version"
min_herdr_version = "0.7.5"

[[build]]
version = "ignored"
TOML

	printf '%s\n' "$root"
}

run_task() {
	local root="$1"
	local task="$2"
	shift 2
	(cd "$root" && PATH="$fakebin:/usr/bin:/bin" MISE_PROJECT_ROOT="$root" "$root/mise-tasks/$task" "$@") 2>&1
}

read_version() {
	local root="$1"
	local task="$2"
	(cd "$root" && PATH="$fakebin:/usr/bin:/bin" MISE_PROJECT_ROOT="$root" "$root/mise-tasks/$task" 2>/dev/null)
}

synced_root="$(make_repo synced 1.2.3 1.2.3 1.2.3)"
[[ "$(read_version "$synced_root" version/cargo)" == 1.2.3 ]] || fail 'version/cargo misread Cargo.toml'
[[ "$(read_version "$synced_root" version/lock)" == 1.2.3 ]] || fail 'version/lock misread Cargo.lock'
[[ "$(read_version "$synced_root" version/manifest)" == 1.2.3 ]] || fail 'version/manifest misread herdr-plugin.toml'
run_task "$synced_root" version/check >/dev/null || fail 'synchronized versions reported drift'
run_task "$synced_root" version/check v1.2.3 >/dev/null || fail 'matching tag reported drift'

set +e
mismatch_output="$(run_task "$synced_root" version/check v9.9.9)"
mismatch_status=$?
set -e
[[ "$mismatch_status" -ne 0 ]] || fail 'a tag Cargo does not declare should fail'
assert_contains "$mismatch_output" 'Cargo.toml declares 1.2.3; v9.9.9 expects 9.9.9'

drifted_root="$(make_repo drifted 1.2.3 1.2.1 1.2.2)"
set +e
drift_output="$(run_task "$drifted_root" version/check)"
drift_status=$?
set -e
[[ "$drift_status" -ne 0 ]] || fail 'version drift should fail'
assert_contains "$drift_output" 'Cargo.lock is out of sync'
assert_contains "$drift_output" 'herdr-plugin.toml is out of sync'

run_task "$drifted_root" version/sync >/dev/null || fail 'version/sync left the repository out of sync'
[[ "$(read_version "$drifted_root" version/lock)" == 1.2.3 ]] || fail 'version/sync did not repair Cargo.lock'
[[ "$(read_version "$drifted_root" version/manifest)" == 1.2.3 ]] || fail 'version/sync did not repair herdr-plugin.toml'
[[ "$(read_version "$drifted_root" version/cargo)" == 1.2.3 ]] || fail 'version/sync changed the source of truth'
[[ -z "$(find "$drifted_root" -name '*.next')" ]] || fail 'version/sync left a temporary file behind'

identity_root="$(make_repo identity 1.2.3 1.2.3 1.2.3 stale-package)"
identity_output="$(run_task "$identity_root" version/sync)" || fail "version/sync did not repair the workspace package identity: $identity_output"
[[ "$(read_version "$identity_root" version/lock)" == 1.2.3 ]] || fail 'version/sync did not synchronize the workspace package identity'

bump_root="$(make_repo bump 1.2.3 1.2.3 1.2.3)"
run_task "$bump_root" version/bump v1.3.0 >/dev/null || fail 'version/bump failed'
[[ "$(read_version "$bump_root" version/cargo)" == 1.3.0 ]] || fail 'version/bump did not set Cargo.toml'
[[ "$(read_version "$bump_root" version/lock)" == 1.3.0 ]] || fail 'version/bump did not sync Cargo.lock'
[[ "$(read_version "$bump_root" version/manifest)" == 1.3.0 ]] || fail 'version/bump did not sync herdr-plugin.toml'
run_task "$bump_root" version/check v1.3.0 >/dev/null || fail 'version/bump left the repository unreleasable'

printf 'Versioning tests passed.\n'
