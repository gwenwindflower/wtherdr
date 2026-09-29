#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$repo_root/herdr-plugin.toml"

if ! awk '
  $0 == "[[build]]" { build = 1; next }
  build && /^command = / { exit !($0 == "command = [\"sh\", \"scripts\/install-binary.sh\"]") }
  END { if (!build) exit 1 }
' "$manifest"; then
	printf 'Plugin manifest must install wtherdr through scripts/install-binary.sh.\n' >&2
	exit 1
fi

if ! awk '/^command = \["wtherdr", / { found = 1 } END { exit !found }' "$manifest"; then
	printf 'Plugin manifest declares no commands.\n' >&2
	exit 1
fi

unexpected="$(awk '
  /^command = / && $0 != "command = [\"sh\", \"scripts\/install-binary.sh\"]" && $0 !~ /^command = \["wtherdr", / { print; exit }
' "$manifest")"
if [[ -n "$unexpected" ]]; then
	printf 'Plugin commands must invoke wtherdr from PATH: %s\n' "$unexpected" >&2
	exit 1
fi

printf 'Plugin manifest tests passed.\n'
