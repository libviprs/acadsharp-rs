#!/usr/bin/env bash
#
# The AOT-against-JIT verdict, one line per fixture.
#
#   tools/aot_parity.sh <dump-binary> <fixtures-dir> <expectations-dir> [<out-dir>]
#
# <dump-binary> is the built `dump` example (`cargo build --example dump`, with
# ACADSHARP_NATIVE_DIR pointing at the archive under test and
# ACADSHARP_REQUIRE_NATIVE=1 so an unlinked build is a build error). For every
# <expectations-dir>/<id>.txt that has a <fixtures-dir>/<id>.dwg it runs the
# example on the fixture and compares the output with the recording using
# plain `diff`: the recording is six-decimal text with a fixed line shape and
# the rule it was recorded under is string equality, so `diff` is the
# comparator and the first differing line is the first differing record.
#
# Two comparisons per fixture, because the wire cannot carry one thing the
# recording holds. The recording was dumped from the flattener's in-memory
# primitives, and a Warning there has `flags=1` when it was raised inside an
# expanded insertion. The wire's Warning record is code, entity handle and
# message only, with no flags field, so a decode through the shim can never
# print that 1. That is a property of the wire, not of AOT against JIT, so:
#   strict  plain diff, flags included
#   wire    the same diff with `flags=N` on Warning lines set to 0 on both sides
# The `wire` verdict is the one that isolates the runtime; `strict` is kept so
# the loss is visible rather than normalised away.
#
# Output, to stdout and to <out-dir>/verdicts.tsv:
#   <id> <TAB> <strict> <TAB> <wire> <TAB> <first differing record under wire,
#   or under strict when only strict differs, or the error>
# each verdict MATCH, DIFF or ERROR. Exit status is 0 when every recorded
# fixture matched under `wire`, 1 when any did not.
#
# A recording whose fixture is missing is reported as ERROR, never skipped.
set -uo pipefail

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
  echo "usage: $0 <dump-binary> <fixtures-dir> <expectations-dir> [<out-dir>]" >&2
  exit 2
fi
dump=$1; fixtures=$2; expect=$3; out=${4:-$(mktemp -d)}
mkdir -p "$out"
: > "$out/verdicts.tsv"

norm() { sed -E 's/^([0-9]+ Warning handle=[0-9A-Fa-f]+) flags=[0-9]+/\1 flags=0/' "$1"; }

# First differing record between two files, the way CanonicalDump.FirstDifference
# names it: the 0-based index (line number minus one) and the two lines.
first_diff() {
  local hunk line exp act
  hunk=$(diff "$1" "$2" | head -n 1)
  line=${hunk%%[acd,]*}
  # `4a5,6`: the recording has nothing from line 5 on, so line 5 is the first
  # differing record.
  case "$hunk" in *a*) line=$((line + 1)) ;; esac
  exp=$(sed -n "${line}p" "$1" | cut -c1-200)
  act=$(sed -n "${line}p" "$2" | cut -c1-200)
  printf 'record %05d expected: %s | actual: %s' "$((line - 1))" "${exp:-<end of stream>}" "${act:-<end of stream>}"
}

bad=0
for rec in "$expect"/*.txt; do
  id=$(basename "$rec" .txt)
  dwg="$fixtures/$id.dwg"
  if [ ! -f "$dwg" ]; then
    printf '%s\tERROR\tERROR\tno fixture %s\n' "$id" "$dwg" >> "$out/verdicts.tsv"
    bad=1
    continue
  fi
  if ! "$dump" "$dwg" > "$out/$id.txt" 2> "$out/$id.err"; then
    printf '%s\tERROR\tERROR\t%s\n' "$id" "$(head -n 1 "$out/$id.err")" >> "$out/verdicts.tsv"
    bad=1
    continue
  fi
  strict=MATCH; wire=MATCH; why=""
  if ! diff -q "$rec" "$out/$id.txt" > /dev/null; then
    strict=DIFF
    norm "$rec" > "$out/$id.rec.norm"; norm "$out/$id.txt" > "$out/$id.act.norm"
    if diff -q "$out/$id.rec.norm" "$out/$id.act.norm" > /dev/null; then
      why=$(first_diff "$rec" "$out/$id.txt")
    else
      wire=DIFF; bad=1
      why=$(first_diff "$out/$id.rec.norm" "$out/$id.act.norm")
    fi
  fi
  printf '%s\t%s\t%s\t%s\n' "$id" "$strict" "$wire" "$why" >> "$out/verdicts.tsv"
done
cat "$out/verdicts.tsv"
exit $bad
