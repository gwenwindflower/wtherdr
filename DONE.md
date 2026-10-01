# wtherdr DONE

## Phase 1: Rust project setup

**Requirements**: dev-R001, dev-R002, dev-R003, dev-R004, dev-R005, dev-R006

### Complete the Rust template integration

- [x] Compare shared tasks and the Rust kit with _tool
- [x] Test and implement version hooks, packaging, and guarded crate publication
- [x] Align CI, tool declarations, and release targets
- [x] Run gates and record intentional template differences

Reviewed _tool at 8eaa822099795ad42fb4e74a5348e71ad2030cf5 and the project-workflows Rust kit. Adopted the version hook interface, release packaging and artifact recovery tasks, release preflight checks, ARM Linux builds, problem matchers, weekly CI cache rotation, and current pinned mise-action. Crate bootstrap and OIDC publication follow Heraldr's guarded publication contract, with all binary assets required before publication.

Shared label, ruleset, and recovery regressions passed in _tool/template. Removed duplicate local repository tests; retained Cargo, packaging, publication, manifest, installer, and task-selection coverage. Intentional differences are Herdr manifest versioning, binaries in target/release, plugin-owned source compilation fallback, open contribution policy, and no Homebrew publishing.

The full release:check gate passed with 28 Rust tests, shell suites, optimized and packaged builds, and workflow audits. Packaging tests verify archive contents, executable permissions, and checksums. First crates.io publication and trusted publisher configuration remain human operations; OIDC stays disabled until CRATES_IO_PUBLISHING is enabled.
