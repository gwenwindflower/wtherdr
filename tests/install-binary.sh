#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sandbox="$(mktemp -d "${TMPDIR:-/tmp}/wtherdr-installer.XXXXXX")"
trap 'rm -rf "$sandbox"' EXIT

make_case() {
	local name="$1"
	local root="$sandbox/$name"
	mkdir -p "$root/scripts" "$root/fakebin"
	cp "$repo_root/scripts/install-binary.sh" "$root/scripts/install-binary.sh"
	printf '%s\n' '[package]' 'name = "wtherdr"' 'version = "0.0.1"' >"$root/Cargo.toml"
	: >"$root/install.log"
	printf '0\n' >"$root/binstall-exit"
	printf 'current\n' >"$root/install-result"
	printf '%s\n' "$root"
}

write_wtherdr() {
	local path="$1"
	local version="$2"
	cat >"$path" <<SCRIPT
#!/bin/sh
printf '%s\\n' 'wtherdr $version'
SCRIPT
	chmod +x "$path"
}

write_cargo() {
	local root="$1"
	cat >"$root/fakebin/cargo" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail

case "$1" in
  pkgid)
    printf '%s\n' 'path+file:///tmp/wtherdr#wtherdr@0.0.1'
    ;;
  install)
    printf '%s\n' "$*" >>"$TEST_LOG"
    case "$TEST_INSTALL_RESULT" in
      current)
        cat >"$TEST_BIN/wtherdr" <<'BINARY'
#!/bin/sh
printf '%s\n' 'wtherdr 0.0.1'
BINARY
        chmod +x "$TEST_BIN/wtherdr"
        ;;
      stale)
        cat >"$TEST_BIN/wtherdr" <<'BINARY'
#!/bin/sh
printf '%s\n' 'wtherdr 0.0.0'
BINARY
        chmod +x "$TEST_BIN/wtherdr"
        ;;
      missing) ;;
      *)
        printf 'Unexpected install result: %s\n' "$TEST_INSTALL_RESULT" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    printf 'Unexpected Cargo command: %s\n' "$*" >&2
    exit 1
    ;;
esac
SCRIPT
	chmod +x "$root/fakebin/cargo"
}

write_binstall() {
	local root="$1"
	local exit_code="${2:-0}"
	cat >"$root/fakebin/cargo-binstall" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$TEST_LOG"
if [[ "$TEST_BINSTALL_EXIT" != 0 ]]; then
  exit "$TEST_BINSTALL_EXIT"
fi
cat >"$TEST_BIN/wtherdr" <<'BINARY'
#!/bin/sh
printf '%s\n' 'wtherdr 0.0.1'
BINARY
chmod +x "$TEST_BIN/wtherdr"
SCRIPT
	chmod +x "$root/fakebin/cargo-binstall"
	printf '%s\n' "$exit_code" >"$root/binstall-exit"
}

run_installer() {
	local root="$1"
	(
		cd "$root"
		TEST_BIN="$root/fakebin" \
			TEST_BINSTALL_EXIT="$(<"$root/binstall-exit")" \
			TEST_INSTALL_RESULT="$(<"$root/install-result")" \
			TEST_LOG="$root/install.log" \
			PATH="$root/fakebin:/usr/bin:/bin" \
			./scripts/install-binary.sh
	)
}

current_root="$(make_case current)"
write_cargo "$current_root"
write_wtherdr "$current_root/fakebin/wtherdr" 0.0.1
write_binstall "$current_root"
run_installer "$current_root"
[[ ! -s "$current_root/install.log" ]]

install_root="$(make_case install)"
write_cargo "$install_root"
write_binstall "$install_root"
run_installer "$install_root"
[[ "$(<"$install_root/install.log")" == 'wtherdr --manifest-path Cargo.toml --strategies crate-meta-data --locked --force --no-confirm' ]]
[[ "$("$install_root/fakebin/wtherdr" --version)" == 'wtherdr 0.0.1' ]]
[[ ! -e "$install_root/target/release/wtherdr" ]]

source_root="$(make_case source)"
write_cargo "$source_root"
run_installer "$source_root" >"$source_root/output.log" 2>&1
[[ "$(<"$source_root/install.log")" == 'install --path . --locked --force' ]]
grep -Eq 'Cargo Binstall.*https://github.com/gwenwindflower/wtherdr#install' "$source_root/output.log"

fallback_root="$(make_case fallback)"
write_cargo "$fallback_root"
write_binstall "$fallback_root" 1
run_installer "$fallback_root" >"$fallback_root/output.log" 2>&1
[[ "$(<"$fallback_root/install.log")" == $'wtherdr --manifest-path Cargo.toml --strategies crate-meta-data --locked --force --no-confirm\ninstall --path . --locked --force' ]]

missing_cargo_root="$(make_case missing-cargo)"
if (cd "$missing_cargo_root" && PATH="$missing_cargo_root/fakebin" ./scripts/install-binary.sh) >"$missing_cargo_root/output.log" 2>&1; then
	printf 'Installer succeeded without Cargo.\n' >&2
	exit 1
fi
grep -Eq 'Cargo.*https://github.com/gwenwindflower/wtherdr#install' "$missing_cargo_root/output.log"
grep -Fq 'https://www.rust-lang.org/tools/install' "$missing_cargo_root/output.log"

missing_path_root="$(make_case missing-path)"
write_cargo "$missing_path_root"
printf 'missing\n' >"$missing_path_root/install-result"
if run_installer "$missing_path_root" >"$missing_path_root/output.log" 2>&1; then
	printf 'Installer succeeded without resolving wtherdr from PATH.\n' >&2
	exit 1
fi
grep -Fq 'Cargo installed wtherdr, but it is not on PATH' "$missing_path_root/output.log"

stale_result_root="$(make_case stale-result)"
write_cargo "$stale_result_root"
printf 'stale\n' >"$stale_result_root/install-result"
if run_installer "$stale_result_root" >"$stale_result_root/output.log" 2>&1; then
	printf 'Installer accepted a stale installed binary.\n' >&2
	exit 1
fi
grep -Fq 'reported wtherdr 0.0.0; expected wtherdr 0.0.1' "$stale_result_root/output.log"

printf 'Binary installer tests passed.\n'
