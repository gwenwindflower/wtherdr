#!/bin/sh

set -eu

install_docs=https://github.com/gwenwindflower/wtherdr#install
if ! command -v cargo >/dev/null 2>&1; then
	printf 'Cargo is required to install wtherdr: https://www.rust-lang.org/tools/install (plugin requirements: %s).\n' "$install_docs" >&2
	exit 1
fi

plugin_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$plugin_root"

package_id=$(cargo pkgid --manifest-path Cargo.toml)
version=${package_id##*#}
version=${version##*@}
version=${version##*:}
expected_version="wtherdr $version"

is_current() {
	[ -x "$1" ] || return 1
	installed_version=$("$1" --version 2>/dev/null) || return 1
	[ "$installed_version" = "$expected_version" ]
}

if path_binary=$(command -v wtherdr 2>/dev/null) && is_current "$path_binary"; then
	printf 'wtherdr %s is already installed at %s.\n' "$version" "$path_binary"
	exit 0
fi

if command -v cargo-binstall >/dev/null 2>&1; then
	printf 'Downloading wtherdr %s with Cargo Binstall.\n' "$version"
	if cargo-binstall wtherdr \
		--manifest-path Cargo.toml \
		--strategies crate-meta-data \
		--locked \
		--force \
		--no-confirm &&
		path_binary=$(command -v wtherdr 2>/dev/null) &&
		is_current "$path_binary"; then
		exit 0
	fi
	printf 'No compatible release artifact found; installing wtherdr from source.\n' >&2
else
	printf 'Cargo Binstall is unavailable; installing wtherdr from source. Install Cargo Binstall for faster installs: %s\n' "$install_docs" >&2
fi

cargo install --path . --locked --force
if ! path_binary=$(command -v wtherdr 2>/dev/null); then
	printf "Cargo installed wtherdr, but it is not on PATH. Add \$CARGO_HOME/bin (normally ~/.cargo/bin) to PATH.\n" >&2
	exit 1
fi
if ! is_current "$path_binary"; then
	installed_version=$("$path_binary" --version 2>/dev/null || printf 'unavailable')
	printf 'wtherdr on PATH reported %s; expected %s.\n' "$installed_version" "$expected_version" >&2
	exit 1
fi
