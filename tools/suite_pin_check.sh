#!/usr/bin/env bash
#
# The merged-only rule for SUITE_REV, inverted from the suite's COUNTERPART_REV
# rule, and pulled out of .github/workflows/ci.yml so it can actually be tested.
#
# The `main` arm cannot be rehearsed by pushing a branch: to get a run whose
# GITHUB_REF_NAME is `main` you have to merge, which is exactly the thing this
# rule is supposed to gate. So the workflow calls this, and
# tests/suite_pin_check.rs drives it over every ref kind crossed with every
# compare status.
#
# Usage:
#   GITHUB_REF_NAME=<ref> tools/suite_pin_check.sh <compare-status> [<suite-sha>]
#
# <compare-status> is the `status` field of
# `gh api repos/<owner>/acadsharp-rs-tests/compare/main...<sha>`, one of
# `identical`, `behind`, `ahead` or `diverged`. `identical` and `behind` mean
# the pin is on the suite's `main`; `ahead` and `diverged` mean it is not.
# Anything else is refused rather than assumed benign, because a renamed field
# or a broken query would otherwise read as "merged" and pass silently.
#
# Exit codes are split on purpose:
#   0  the pin is allowed on this ref
#   1  the pin is refused (a policy decision)
#   2  the script was called wrong (a wiring mistake)
#
# Second mode, which asks git instead of the compare API (so `cargo test` and
# the Docker path get the check without a token):
#
#   GITHUB_REF_NAME=<ref> $0 reachable <git-dir> <sha> [<pin-file>]
#
# <git-dir> holds the other repo's main (refs/remotes/origin/main, or
# refs/heads/main) and the pin, and the pin must NOT be shallow there, or
# merge-base cannot tell an open PR head from an unrelated history. It keeps
# the compare arm's three-way split:
#   - the pin is an ancestor of main: allowed on every ref;
#   - it shares history with main but is not an ancestor (an open PR head, or a
#     rewrite that kept part of the history): refused on main, allowed with a
#     notice on a PR ref;
#   - no common ancestor at all: refused on every ref, because an unrelated
#     history can never become mergeable. The message names the commit on main
#     with the same tree, which is where a rewrite means the pin should move.
# <pin-file> (default SUITE_REV) only changes the file the annotation names.
set -euo pipefail

reachable() {
  if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "::error::suite_pin_check.sh reachable wants <git-dir> <sha> [<pin-file>], but got $# argument(s)"
    exit 2
  fi
  local gitdir="$1" pin="$2" file="${3-SUITE_REV}" ref_name="${GITHUB_REF_NAME-}"
  if [ -z "$ref_name" ]; then
    echo "::error::GITHUB_REF_NAME is empty or unset, so suite_pin_check.sh cannot tell a PR branch from \`main\`; refusing to guess"
    exit 2
  fi
  local g=(git --git-dir "$gitdir") main="" cand
  for cand in refs/remotes/origin/main refs/heads/main; do
    if "${g[@]}" rev-parse --verify --quiet "$cand^{commit}" >/dev/null; then
      main="$cand"
      break
    fi
  done
  if [ -z "$main" ]; then
    echo "::error::no main in $gitdir (looked for refs/remotes/origin/main and refs/heads/main)"
    exit 2
  fi
  if ! "${g[@]}" cat-file -e "$pin^{commit}" 2>/dev/null; then
    echo "::error file=$file::$pin is not a commit in $gitdir; it was never fetched, or it does not exist there"
    exit 1
  fi
  if "${g[@]}" merge-base --is-ancestor "$pin" "$main"; then
    echo "$file $pin is an ancestor of main, so the merged-only rule is satisfied."
    exit 0
  fi
  local tree twin=""
  tree="$("${g[@]}" rev-parse "$pin^{tree}")"
  twin="$("${g[@]}" log --format='%H %T' "$main" | awk -v t="$tree" '$2 == t { print $1; exit }')"
  local hint=""
  if [ -n "$twin" ]; then
    hint=" The commit on main with the same tree is $twin; that is a rewrite, and the pin should move there."
  fi
  if ! "${g[@]}" merge-base "$pin" "$main" >/dev/null 2>&1; then
    echo "::error file=$file::$pin shares no history with main, so no branch reaches it and it can never merge.$hint"
    exit 1
  fi
  if [ "$ref_name" = "main" ]; then
    echo "::error file=$file::$pin is not an ancestor of main and this run is on main. The follow-up PR is owed: move $file onto the merge commit.$hint"
    exit 1
  fi
  echo "::notice file=$file::$pin is not on main yet, but it shares history with it. That is allowed on $ref_name, because a paired change needs the other branch first. Once it merges, a one-line PR must move $file onto its merge commit or main goes red.$hint"
  exit 0
}

if [ "${1-}" = "reachable" ]; then
  shift
  reachable "$@"
fi

usage() {
  echo "usage: GITHUB_REF_NAME=<ref> $0 <compare-status> [<suite-sha>] | reachable <git-dir> <sha> [<pin-file>]" >&2
}

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "::error::suite_pin_check.sh wants a compare status and an optional sha, but got $# argument(s)"
  usage
  exit 2
fi

status="$1"
sha="${2-}"
ref_name="${GITHUB_REF_NAME-}"

if [ -z "$status" ]; then
  echo "::error::suite_pin_check.sh got an empty compare status; the \`gh api ... -q .status\` query that feeds it returned nothing"
  usage
  exit 2
fi

# An unset variable reads as the empty string, and the empty string has already
# been read as "the default branch" once in this org (libviprs-org's repository
# variable). Empty is not `main` and it is not a PR branch either, so refuse
# rather than pick one.
if [ -z "$ref_name" ]; then
  echo "::error::GITHUB_REF_NAME is empty or unset, so suite_pin_check.sh cannot tell a PR branch from \`main\`; refusing to guess"
  usage
  exit 2
fi

[ -n "$sha" ] || sha="(no sha given)"

case "$status" in
  identical | behind)
    echo "SUITE_REV $sha is on the suite's main (compare says $status), so the merged-only rule is satisfied."
    exit 0
    ;;
  ahead | diverged)
    # Handled below, where the ref decides.
    ;;
  *)
    echo "::error file=SUITE_REV::unknown compare status '$status' for SUITE_REV $sha; expected identical, behind, ahead or diverged. Refusing to treat an unrecognised status as merged."
    exit 1
    ;;
esac

if [ "$ref_name" = "main" ]; then
  echo "::error file=SUITE_REV::SUITE_REV $sha is not on the suite's main (compare says $status) and this run is on main. That means the follow-up PR is owed: move SUITE_REV onto the acadsharp-rs-tests merge commit and this goes green."
  exit 1
fi

echo "::notice file=SUITE_REV::SUITE_REV $sha is not on the suite's main yet (compare says $status). That is allowed on $ref_name, because a paired change needs the suite branch to exist first. Once the suite PR merges, a one-line PR must move SUITE_REV onto its merge commit or main goes red."
exit 0
