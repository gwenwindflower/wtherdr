#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sandbox="$(mktemp -d "${TMPDIR:-/tmp}/wtherdr-rulesets.XXXXXX")"
trap 'rm -rf "$sandbox"' EXIT
mkdir -p "$sandbox/bin" "$sandbox/.github/rulesets"
cp "$repo_root/.github/rulesets/main.json" "$sandbox/.github/rulesets/main.json"
cat >"$sandbox/bin/gh" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
case "$1 $2" in
  'repo view') printf 'gwenwindflower/wtherdr\n' ;;
  'run list')
    [[ "$*" == *'--workflow ci.yml'* && "$*" == *'--branch main'* && "$*" == *'--event push'* && "$*" == *'--status completed'* ]]
    if [[ "${TEST_NO_CI:-0}" != 1 ]]; then printf '123\n'; fi ;;
  'run view')
    [[ "$3" == 123 ]]
    printf '%s\n' '["Check","Test (ubuntu-24.04)","Test (macos-15)","Audit workflows"]' ;;
  'api repos/gwenwindflower/wtherdr/commits/main/check-runs')
    printf '%s\n' '["Check","Test (ubuntu-24.04)","Test (macos-15)","Audit workflows","Upload release assets"]' ;;
  'api repos/gwenwindflower/wtherdr/rulesets') : ;;
  'api --method') cat >"$TEST_RULESET" ;;
  *) printf 'Unexpected gh invocation: %s\n' "$*" >&2; exit 1 ;;
esac
SCRIPT
chmod +x "$sandbox/bin/gh"
export TEST_RULESET="$sandbox/ruleset.json"
run_rulesets() {
  MISE_PROJECT_ROOT="$sandbox" PATH="$sandbox/bin:$PATH" bash "$repo_root/mise-tasks/repo/rulesets"
}
run_rulesets
jq -e '
  [.rules[] | select(.type == "required_status_checks") | .parameters.required_status_checks[].context] | sort
  == (["Check", "Test (ubuntu-24.04)", "Test (macos-15)", "Audit workflows"] | sort)
' "$TEST_RULESET" >/dev/null
rm "$TEST_RULESET"
export TEST_NO_CI=1
if run_rulesets >"$sandbox/output" 2>&1; then
  printf 'Ruleset provisioning accepted a repository without CI.\n' >&2
  exit 1
fi
[[ ! -e "$TEST_RULESET" ]]
grep -Fq 'No completed CI run' "$sandbox/output"
printf 'Repository ruleset tests passed.\n'
