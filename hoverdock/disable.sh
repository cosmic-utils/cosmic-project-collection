#!/usr/bin/env bash
# Put COSMIC's own app list back in the dock and restart the panel.
#
# Safe to run at any time, including when HoverDock is not installed.
set -euo pipefail

if command -v hoverdock >/dev/null 2>&1; then
    hoverdock --uninstall
elif [ -x "$HOME/.local/bin/hoverdock" ]; then
    "$HOME/.local/bin/hoverdock" --uninstall
else
    # Even with the binary gone, the plugin list can be put right by hand.
    plugins="$HOME/.config/cosmic/com.system76.CosmicPanel.Dock/v1/plugins_center"
    if [ -f "$plugins" ]; then
        sed -i 's/com\.techy\.HoverDock/com.system76.CosmicAppList/' "$plugins"
        echo "restored $plugins"
    fi
    rm -f "$HOME/.local/share/applications/com.techy.HoverDock.desktop"
fi

echo "restarting cosmic-panel..."
pkill -x cosmic-panel || true
for _ in 1 2 3 4 5 6 7 8 9 10; do
    sleep 0.5
    if pgrep -x cosmic-panel >/dev/null; then
        echo "the panel is back."
        exit 0
    fi
done

setsid "$(command -v cosmic-panel)" >/dev/null 2>&1 &
sleep 1
pgrep -x cosmic-panel >/dev/null && echo "the panel is back." || {
    echo "cosmic-panel is not running. Log out and back in to recover." >&2
    exit 1
}
