#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sandbox="$(mktemp -d "${TMPDIR:-/tmp}/wtherdr-labels.XXXXXX")"
trap 'rm -rf "$sandbox"' EXIT
mkdir -p "$sandbox/bin" "$sandbox/.github"
cat >"$sandbox/.github/labels.yml" <<'YAML'
- name: "type: bug"
  color: "ea999c"
  description: "Something isn't working"
- name: "help wanted"
  color: "85c1dc"
  description: "Extra attention is needed"
YAML
cat >"$sandbox/bin/gh" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
case "$1 $2" in
  'repo view') printf 'gwenwindflower/wtherdr\n' ;;
  'api repos/gwenwindflower/wtherdr/labels?per_page=100')
    [[ "$*" == *'--paginate'* ]] || exit 1
    [[ "${TEST_LIST_FAIL:-0}" != 1 ]] || exit 1
    cat "$TEST_LABELS" ;;
  'label create')
    [[ "${TEST_CREATE_FAIL:-0}" != 1 ]] || exit 1
    if grep -Fxq -- "$3" "$TEST_LABELS"; then
      printf 'Existing label was not skipped: %s\n' "$3" >&2
      exit 1
    fi
    printf '%s\n' "$3" >>"$TEST_LABELS" ;;
  'label delete')
    grep -Fxq 'help wanted' "$TEST_LABELS" || exit 1
    grep -Fxv -- "$3" "$TEST_LABELS" >"$TEST_LABELS.next"
    mv "$TEST_LABELS.next" "$TEST_LABELS" ;;
  *) printf 'Unexpected gh invocation: %s\n' "$*" >&2; exit 1 ;;
esac
SCRIPT
chmod +x "$sandbox/bin/gh"
export TEST_LABELS="$sandbox/labels"
run_labels() {
  MISE_PROJECT_ROOT="$sandbox" PATH="$sandbox/bin:$PATH" bash "$repo_root/mise-tasks/repo/labels"
}
reset_labels() {
  printf '%s\n' 'type: bug' 'bug' 'enhancement' >"$TEST_LABELS"
}
reset_labels
run_labels
printf '%s\n' 'type: bug' 'help wanted' >"$sandbox/expected"
cmp "$sandbox/expected" "$TEST_LABELS"
run_labels
cmp "$sandbox/expected" "$TEST_LABELS"

: >"$TEST_LABELS"
run_labels
cmp "$sandbox/expected" "$TEST_LABELS"

for failure in TEST_LIST_FAIL TEST_CREATE_FAIL; do
  reset_labels
  cp "$TEST_LABELS" "$sandbox/expected"
  export "${failure}=1"
  if run_labels >"$sandbox/output" 2>&1; then
    printf 'Label reconciliation accepted %s.\n' "$failure" >&2
    exit 1
  fi
  unset "$failure"
  cmp "$sandbox/expected" "$TEST_LABELS"
done

: >"$sandbox/.github/labels.yml"
if run_labels >"$sandbox/output" 2>&1; then
  printf 'Label reconciliation accepted an empty declaration.\n' >&2
  exit 1
fi
cmp "$sandbox/expected" "$TEST_LABELS"
printf 'Repository label tests passed.\n'
