#!/usr/bin/env bash
# Extracts the assets the Bevy sandbox needs from YOUR copy of Red Faction Guerrilla (Steam, v1.0.2.1).
# macOS / Linux counterpart of ExtractAssets.bat. The game is Windows-only, so on a Mac it is usually
# installed through CrossOver, Whisky or Wine, or copied over from a Windows PC; any folder holding
# RFG.exe and build/pc/cache works.
# Nothing is downloaded except the Python packages numpy, Pillow and scipy (into LocalData/venv).
#
# Usage: ./ExtractAssets.sh ["/path/to/Red Faction Guerrilla"] [extract_all.py options]
#   options: --force (redo finished steps)   --clean (delete LocalData/Extracted when done)
#            --only STEP,STEP (misc,items,anims,player,chunks,assets,tables,build)
#            --chunks NAME ... (build only those chunks, e.g. --chunks 0203house01_a)
# Needs about 3 GB of free disk space and takes roughly an hour. It can be interrupted and re-run.
# The game folder is remembered in LocalData/game_dir.txt.

set -u
cd "$(dirname "$0")"

fail() {
    echo
    echo "Extraction did not finish. Fix the problem above and run ExtractAssets.sh again; finished steps are skipped."
    exit 1
}

valid_game() { [ -f "$1/RFG.exe" ] && [ -d "$1/build/pc/cache" ]; }

# ---- arguments: an optional game folder first, everything else goes to extract_all.py ----
GAME=""
if [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; then
    GAME="${1%/}"
    shift
fi

# ---- Python 3.9+ ----
PY=""
for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 &&
        "$candidate" -c "import sys; sys.exit(sys.version_info < (3, 9))" >/dev/null 2>&1; then
        PY="$candidate"
        break
    fi
done
if [ -z "$PY" ]; then
    echo "Python 3.9 or newer was not found. Install it (macOS: 'brew install python' or https://www.python.org/downloads/), then run this again."
    fail
fi

# ---- game folder: argument, remembered path, Steam / Wine bottles, or a folder picker ----
if [ -z "$GAME" ] && [ -f LocalData/game_dir.txt ]; then
    GAME="$(head -n 1 LocalData/game_dir.txt)"
fi
if [ -n "$GAME" ] && ! valid_game "$GAME"; then
    GAME=""
fi
if [ -z "$GAME" ]; then
    GAME="$("$PY" -I Extractors/find_game.py)"
fi
while [ -z "$GAME" ] || ! valid_game "$GAME"; do
    [ -n "$GAME" ] && echo "That folder does not contain RFG.exe and build/pc/cache. Re-Mars-tered is not supported."
    [ -z "$GAME" ] && echo "Could not find Red Faction Guerrilla automatically."
    PROMPT="Select your Red Faction Guerrilla install folder (containing RFG.exe)"
    if [ "$(uname)" = "Darwin" ]; then
        GAME="$(osascript -e "POSIX path of (choose folder with prompt \"$PROMPT\")" 2>/dev/null)"
    elif command -v zenity >/dev/null 2>&1; then
        GAME="$(zenity --file-selection --directory --title="$PROMPT" 2>/dev/null)"
    else
        read -r -p "Path to the game folder (empty to cancel): " GAME
    fi
    if [ -z "$GAME" ]; then
        echo "No game folder selected. Extraction stopped."
        exit 1
    fi
    GAME="${GAME%/}"
done
mkdir -p LocalData
printf '%s\n' "$GAME" > LocalData/game_dir.txt
echo "Game folder: $GAME"

# ---- virtual environment with the three packages ----
VPY=LocalData/venv/bin/python
if [ ! -x "$VPY" ]; then
    echo "Creating LocalData/venv ..."
    "$PY" -m venv LocalData/venv || fail
fi
if ! "$VPY" -c "import numpy, PIL, scipy" >/dev/null 2>&1; then
    echo "Installing numpy, Pillow and scipy ..."
    "$VPY" -m pip install --disable-pip-version-check numpy pillow scipy || fail
fi

# ---- extract ----
"$VPY" Extractors/extract_all.py --game "$GAME" "$@" || fail
echo
echo "Extraction command completed. See the verification or selected-step summary above."
