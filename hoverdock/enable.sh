#!/usr/bin/env bash
# Switch the COSMIC dock over to HoverDock.
#
# Installs the binary, registers the applet, swaps it in for cosmic-app-list
# and restarts the panel. Everything it changes is undone by ./disable.sh.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
binary="$here/target/release/hoverdock"

if [ ! -x "$binary" ]; then
    echo "no build yet — run:  cargo build --release" >&2
    exit 1
fi

mkdir -p "$HOME/.local/bin"
install -m 755 "$binary" "$HOME/.local/bin/hoverdock"
echo "installed $HOME/.local/bin/hoverdock"

"$HOME/.local/bin/hoverdock" --install

# The panel reads its plugin list once, at startup. cosmic-session normally
# brings it straight back; if it does not, start it here rather than leave the
# desktop without a panel.
echo "restarting cosmic-panel..."
pkill -x cosmic-panel || true
for _ in 1 2 3 4 5 6 7 8 9 10; do
    sleep 0.5
    if pgrep -x cosmic-panel >/dev/null; then
        echo "the panel is back."
        exit 0
    fi
done

echo "the session did not restart it; starting it here."
setsid "$(command -v cosmic-panel)" >/dev/null 2>&1 &
sleep 1
pgrep -x cosmic-panel >/dev/null && echo "the panel is back." || {
    echo "cosmic-panel is not running. Log out and back in to recover." >&2
    exit 1
}
