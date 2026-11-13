# Workflow actions

wtherdr presents Worktrunk actions in Herdr popups while retaining Worktrunk's interactive prompts and approval rules.

- **wf-R001** — Completed actions other than successful creation present a fresh result popup that remains visible until Enter or Esc.
- **wf-R002** — The result popup animates while Herdr workspace cleanup runs and settles on success or failure when cleanup finishes.
- **wf-R003** — A failed action keeps its operation, checkout, and available diagnostic details readable in the result popup.
- **wf-R004** — Removing a checkout defers closing its Herdr workspace until the run popup has exited.
- **wf-R005** — After Worktrunk's switch picker exits, wtherdr closes registered workspaces whose checkouts were present before the picker and are confirmed absent afterward, including after cancellation or failure.
- **wf-R006** — Cancelling an action without changing worktrees closes its popup without showing a success result.
- **wf-R007** — Rebase offers existing local and remote branches in a filterable list, with `main` selected by default when it exists.
- **wf-R008** — If `main` is absent, rebase requires an explicit selection instead of choosing another branch automatically.
- **wf-R009** — Both branch and hook pickers accept arrows, Ctrl-J/K, and Ctrl-N/P for navigation, Enter to select, and Esc to cancel.
- **wf-R010** — The hook picker offers configured user and project Worktrunk hooks and runs only the selected hook in the current checkout.
- **wf-R011** — Hook execution waits for the selected command to finish and retains Worktrunk's approval prompts.
- **wf-R012** — Rebase conflicts remain available for `git rebase --continue` or `git rebase --abort` in the affected checkout.
- **wf-R013** — Successful creation closes after workspace reconciliation without presenting a success popup.
