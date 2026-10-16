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
# Output, to stdout and to <out-dir>/verdicts.tsv:
#   <id> <TAB> MATCH|DIFF|ERROR <TAB> <first differing record, or the error>
# Exit status is 0 when every recorded fixture matched, 1 when any did not.
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

bad=0
for rec in "$expect"/*.txt; do
  id=$(basename "$rec" .txt)
  dwg="$fixtures/$id.dwg"
  if [ ! -f "$dwg" ]; then
    printf '%s\tERROR\tno fixture %s\n' "$id" "$dwg" >> "$out/verdicts.tsv"
    bad=1
    continue
  fi
  if ! "$dump" "$dwg" > "$out/$id.txt" 2> "$out/$id.err"; then
    printf '%s\tERROR\t%s\n' "$id" "$(head -n 1 "$out/$id.err")" >> "$out/verdicts.tsv"
    bad=1
    continue
  fi
  if diff -q "$rec" "$out/$id.txt" > /dev/null; then
    printf '%s\tMATCH\t\n' "$id" >> "$out/verdicts.tsv"
  else
    # The first differing record, the way CanonicalDump.FirstDifference names
    # it: the 0-based index (line number minus one) and the two lines. A
    # recording that is a prefix of the dump (or the other way round) differs
    # first at the line one side does not have.
    hunk=$(diff "$rec" "$out/$id.txt" | head -n 1)
    line=${hunk%%[acd,]*}
    # `4a5,6`: the recording has nothing from line 5 on, so line 5 is the first
    # differing record.
    case "$hunk" in *a*) line=$((line + 1)) ;; esac
    idx=$((line - 1))
    exp=$(sed -n "${line}p" "$rec" | cut -c1-200)
    act=$(sed -n "${line}p" "$out/$id.txt" | cut -c1-200)
    printf '%s\tDIFF\trecord %05d expected: %s | actual: %s\n' "$id" "$idx" "${exp:-<end of stream>}" "${act:-<end of stream>}" >> "$out/verdicts.tsv"
    bad=1
  fi
done
cat "$out/verdicts.tsv"
exit $bad
