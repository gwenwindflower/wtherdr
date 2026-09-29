# Contributing

Use Discussions for questions and open-ended ideas. File a bug with a reproducible example, the expected result, and your wtherdr, Herdr, and Worktrunk versions. Search existing issues and discussions first. Report security vulnerabilities through GitHub's private vulnerability reporting.

For substantial changes, discuss the scope before implementing. Keep each pull request focused, use an imperative Conventional Commit title under 70 characters, and explain the resulting behavior and how you verified it.

Run `mise run hooks:install` after cloning and `mise run check` before submitting. Write a failing test before changing behavior. Keep renderable and parseable logic pure so tests do not require a live Herdr session. For popup changes, run `mise run dev:reload` and try the workflow in Herdr.

Worktrunk owns worktree operations and hook approvals; wtherdr owns popup presentation and Herdr workspace reconciliation. Preserve native interactive output and actionable errors when one side succeeds and the other fails.
