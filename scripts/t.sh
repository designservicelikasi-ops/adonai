#!/bin/sh
# Run cargo with a compact summary: compile errors, test failures and totals only.
. "$HOME/.cargo/env" 2>/dev/null
LOG="${TMPDIR:-/tmp}/aegis-cargo-$$.log"
cargo "$@" >"$LOG" 2>&1
STATUS=$?
# Compile errors and code warnings (workspace-manifest warnings are skipped).
awk '
  /^(error|warning)/ { show = ($0 !~ /unused workspace dependency/) && ($0 !~ /^warning: `.*generated/) }
  /^(test |running |     Running|   Compiling|    Finished|   Doc-tests|    Checking)/ { show = 0 }
  show { print }
' "$LOG" | head -${LINES_MAX:-80}
grep -E 'FAILED|panicked|assertion|^  left:|^ right:|^test .* \.\.\. (FAILED|failed)' "$LOG" | head -40
grep -E '^test result' "$LOG" | awk '{p+=$4; f+=$6} END {printf "TOTAL: %d passed, %d failed\n", p, f}'
echo "exit=$STATUS"
rm -f "$LOG"
exit $STATUS
