# Repository setup

Run these tasks from a checkout with its GitHub remote configured and `gh` authenticated:

```bash
mise run repo:settings --description "A fast, light herdr-worktrunk integration that is actually good" --topics "herdr-plugin,worktrunk,rust"
mise run repo:labels
mise run repo:rulesets
```

`repo:settings` enables issues, discussions, squash and rebase merges, and automatic head-branch deletion after merge. It disables merge commits, the wiki, and GitHub Projects. Description and topics are optional arguments.

`repo:labels` reads `.github/labels.yml`, creates missing labels, preserves existing declared labels, and deletes labels absent from the declaration.

`repo:rulesets` applies `.github/rulesets/main.json` to the default branch. It requires checks from the latest completed push run of `ci.yml` on `main`; run it after CI has completed at least once. The rules prevent branch deletion, force pushes, and merge commits, with an administrator bypass for the solo-maintainer workflow.

Create the Help, Ideas, and Share discussion categories and enable private vulnerability reporting in GitHub to support the issue chooser links.

`mise run repo:environments` configures the `release` environment for version tags. Binary uploads and crates.io publishing use this environment. After the first crate publication, configure the crates.io trusted publisher for `gwenwindflower/wtherdr`, workflow `release-build.yml`, environment `release`, then set `CRATES_IO_PUBLISHING=true`.

Shared repository-task regressions run in `_tool/template/tests`. Local checks test version synchronization, release packaging, publication guards, and the plugin installer without changing GitHub settings.
