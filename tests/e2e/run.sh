#!/bin/bash
# Run Wisp's end-to-end tests: the real browser on a virtual screen, with
# its own session bus (for the accessibility tree) and a throwaway home.
#
#   tests/e2e/run.sh              every test
#   tests/e2e/run.sh find zoom    tests whose names contain these words
#
# Needs: Xvfb, xdotool, dbus-run-session, python with gi (Atspi 2.0),
# ImageMagick's `import` for screenshots of failures.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --locked --quiet
PYTHON="${PYTHON:-python3}"
if [ -z "${DISPLAY:-}" ]; then
  export DISPLAY=:77
  Xvfb "$DISPLAY" -screen 0 1280x800x24 >/dev/null 2>&1 &
  XVFB=$!
  trap 'kill $XVFB' EXIT
  sleep 1
fi
export NO_AT_BRIDGE=0 GTK_A11Y=atspi
exec dbus-run-session -- "$PYTHON" tests/e2e/test_browser.py "$@"
