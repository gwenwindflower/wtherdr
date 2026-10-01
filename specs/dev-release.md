# Release and repository plumbing

## Requirements

- **dev-R001** — Cargo.toml owns the version; version:read, write, files, and verify keep Cargo.lock and herdr-plugin.toml synchronized.
- **dev-R002** — Automated gates run noninteractive Rust tests, optimized builds, crate package verification, and project integration tests without installing or publishing wtherdr.
- **dev-R003** — Releases attach archives and checksums for x86_64 and aarch64 on Linux and macOS; packaging reads the binary from target/release.
- **dev-R004** — Crate publication requires clean source at the release tag and all four released archives and checksums; first publication uses a confirmed local task, and enabled OIDC publication follows binary upload.
- **dev-R005** — CI audits workflows before lint and tests, registers problem matchers, and uses SHA-pinned actions; publishing jobs use the release environment.
- **dev-R006** — Shared task regressions belong to _tool; local tests cover Cargo, packaging, the plugin installer, and task selection. Homebrew publication is excluded.
