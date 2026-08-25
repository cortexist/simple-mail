#!/usr/bin/env bash
# bench-reading.sh — render every message through the reading pane, several
# times over, and report how the web process's memory moves.
#
#   scripts/bench-reading.sh                 # 3 rounds, all messages
#   AGENTIC_MAIL_BENCH_ROUNDS=5 scripts/bench-reading.sh
#   AGENTIC_MAIL_BENCH_LIMIT=50 scripts/bench-reading.sh   # quick pass
#   AGENTIC_MAIL_CACHE_MODEL=viewer scripts/bench-reading.sh    # A/B a different cache model
#
# Runs `tauri dev` (a debug build against the real ~/.cortexist/mail database)
# with the bench enabled, waits for the runner to write its end marker, stops
# the app, and prints the report. Reading only: nothing is marked read and no
# IMAP write is issued. Remote images in HTML mail DO load, exactly as they do
# when you read — so tracking pixels in messages you never opened will fire.
#
# The running agentic-mail must be closed first: the app is single-instance,
# and a second launch would just focus the existing window.
set -euo pipefail

cd "$(dirname "$0")/.."

if pgrep -x agentic-mail >/dev/null; then
  echo "agentic-mail is running; close it first (single-instance would hand off to it)." >&2
  exit 1
fi

export AGENTIC_MAIL_BENCH=1
# WebKit cache model under test. `browser` is wry's default and what ships;
# `viewer` was measured worse (see install_webview_cache_model). Each model
# logs to its own file.
export AGENTIC_MAIL_CACHE_MODEL="${AGENTIC_MAIL_CACHE_MODEL:-browser}"
export AGENTIC_MAIL_BENCH_LOG="${AGENTIC_MAIL_BENCH_LOG:-$HOME/.cortexist/mail/bench-reading-$AGENTIC_MAIL_CACHE_MODEL.csv}"
: > "$AGENTIC_MAIL_BENCH_LOG"
max_minutes="${AGENTIC_MAIL_BENCH_MAX_MINUTES:-120}"

echo "cache model: $AGENTIC_MAIL_CACHE_MODEL"
echo "log: $AGENTIC_MAIL_BENCH_LOG"
echo "starting tauri dev (debug build; first run compiles)…"
setsid npm run tauri dev > "${AGENTIC_MAIL_BENCH_LOG%.csv}.dev.log" 2>&1 &
dev_pid=$!

cleanup() {
  echo "stopping app…"
  kill -- -"$dev_pid" 2>/dev/null || true
  pkill -x agentic-mail 2>/dev/null || true
}
trap cleanup EXIT INT TERM

deadline=$(( $(date +%s) + max_minutes * 60 ))
while :; do
  if grep -qE '^[0-9]+,(done|stopped|error),' "$AGENTIC_MAIL_BENCH_LOG" 2>/dev/null; then break; fi
  if ! kill -0 "$dev_pid" 2>/dev/null; then
    echo "tauri dev exited before the bench finished; see ${AGENTIC_MAIL_BENCH_LOG%.csv}.dev.log" >&2
    break
  fi
  if (( $(date +%s) > deadline )); then
    echo "gave up after ${max_minutes} min; reporting what was logged" >&2
    break
  fi
  n=$(grep -c ',message,' "$AGENTIC_MAIL_BENCH_LOG" 2>/dev/null || true)
  printf '\r  %s messages sampled…' "${n:-0}"
  sleep 5
done
echo

cleanup
trap - EXIT
node scripts/bench-reading-report.mjs "$AGENTIC_MAIL_BENCH_LOG"
