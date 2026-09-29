#!/usr/bin/env bash

set -euo pipefail
cd "${MISE_PROJECT_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

fail() {
	printf 'FAIL: %s\n' "$1" >&2
	exit 1
}

for task in check test 'test:*' release:check; do
	output="$(mise run --dry-run "$task" 2>&1)"
	if [[ "$output" == *'cargo pretty'* ]]; then
		fail "$task includes interactive Cargo output"
	fi
	if [[ "$output" == *'cargo install'* || "$output" == *'cargo binstall'* || "$output" == *'herdr plugin install'* ]]; then
		fail "$task installs wtherdr instead of testing it"
	fi
	count="$(grep -Fc 'cargo test --all-features --locked' <<<"$output" || true)"
	[[ "$count" == 1 ]] || fail "$task must run the Rust suite exactly once (found ${count:-0})"
	if [[ "$task" == check || "$task" == release:check ]]; then
		count="$(grep -Fc 'prek run --all-files' <<<"$output" || true)"
		[[ "$count" == 1 ]] || fail "$task must run the hook sweep exactly once (found ${count:-0})"
	fi
	[[ "$output" == *'cargo build --locked --release'* ]] || fail "$task omits the optimized build"
	[[ "$output" == *'cargo package --locked --allow-dirty'* ]] || fail "$task omits crate package verification"
	if [[ "$output" == *'cargo publish --locked'* ]]; then
		fail "$task uploads a crate"
	fi
done

printf 'Automated task selection tests passed.\n'
