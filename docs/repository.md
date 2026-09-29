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

Create the Help, Ideas, and Share discussion categories and enable private vulnerability reporting in GitHub to support the issue chooser links. No deployment environment is required by the current release workflow, so there is no `repo:environments` task.

The repository tests use a fake `gh` in temporary directories. They run as part of `mise run check` without changing GitHub settings or labels.
