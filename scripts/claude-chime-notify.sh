#!/bin/bash
# Notification hook for Claude Code - plays a chime sound.
# Drains stdin to maintain hook interface compatibility.
cat > /dev/null

CHIME_FILE="${CLAUDE_CHIME_FILE:-/home/claude/.local/share/sounds/chime.wav}"

if [[ -f "$CHIME_FILE" ]] && command -v pw-play >/dev/null 2>&1; then
    timeout 5 pw-play "$CHIME_FILE" >/dev/null 2>&1 &
elif [[ -f "$CHIME_FILE" ]] && command -v paplay >/dev/null 2>&1; then
    timeout 5 paplay "$CHIME_FILE" >/dev/null 2>&1 &
else
    # Fallback to terminal bell
    printf '\a'
fi
