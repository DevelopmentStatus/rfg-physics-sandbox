#!/usr/bin/env bash
# Builds and opens the Bevy sandbox (buildings district, hammer, demolition panel).
# macOS / Linux counterpart of Play.bat.
# Usage: ./Play.sh [release] [building name]
#   release        optimized build (much faster with many buildings; first build takes a while)
#   building name  start in front of that lot, e.g. ./Play.sh 0203house01_a
# Demolition runs are started from the ImGui "Buildings" window (Esc frees the cursor).

set -u
cd "$(dirname "$0")"

PROFILE=debug
CARGO_FLAGS=()
for arg in "$@"; do
    if [ "$(printf '%s' "$arg" | tr '[:upper:]' '[:lower:]')" = "release" ]; then
        PROFILE=release
        CARGO_FLAGS=(--release)
    else
        export RFG_START_LOT="$arg"
    fi
done

# The game assets are not part of this repository; they are extracted from your own game install.
VPY=LocalData/venv/bin/python
if ! { [ -x "$VPY" ] && "$VPY" Extractors/asset_status.py; }; then
    echo "The game assets have not been extracted yet (LocalData/Assets is missing or incomplete)."
    read -r -p "Run ExtractAssets.sh now? [y/N] " answer
    case "$answer" in [yY]*) ;; *) exit 1 ;; esac
    ./ExtractAssets.sh || exit 1
    "$VPY" Extractors/asset_status.py || exit 1
fi

# rustup installs to ~/.cargo/bin; Homebrew's rustup is keg-only and lives outside PATH by default.
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
for brew_rustup in /opt/homebrew/opt/rustup/bin /usr/local/opt/rustup/bin; do
    [ -d "$brew_rustup" ] && export PATH="$brew_rustup:$PATH"
done

BINARY="LocalData/Build/$PROFILE/sandbox"
if command -v cargo >/dev/null 2>&1; then
    # Building uses one CPU core per job. Ask how many to use (default 4), clamped to 1..all cores.
    # Set CARGO_BUILD_JOBS beforehand to skip the prompt.
    if [ -z "${CARGO_BUILD_JOBS:-}" ]; then
        MAX_CORES="$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 1)"
        BUILD_CORES=$(( MAX_CORES < 4 ? MAX_CORES : 4 ))
        echo
        echo "Building is CPU heavy; the first build compiles every dependency and can take several minutes."
        echo "More cores build faster but leave less CPU for other programs. 1 core is extremely slow."
        read -r -p "CPU cores to use for building [$BUILD_CORES] (you have $MAX_CORES): " CORES_INPUT
        if [ -n "$CORES_INPUT" ]; then
            if [[ "$CORES_INPUT" =~ ^[0-9]+$ ]]; then
                CORES_INPUT=$(( 10#${CORES_INPUT:0:5} ))
                BUILD_CORES=$(( CORES_INPUT > MAX_CORES ? MAX_CORES : CORES_INPUT ))
                [ "$BUILD_CORES" -lt 1 ] && BUILD_CORES=1
            else
                echo "Not a number, using $BUILD_CORES."
            fi
        fi
        export CARGO_BUILD_JOBS="$BUILD_CORES"
    fi
    echo "Building sandbox ($PROFILE) using $CARGO_BUILD_JOBS core(s)..."
    cargo build -p sandbox ${CARGO_FLAGS[@]+"${CARGO_FLAGS[@]}"} || exit 1
elif [ -x "$BINARY" ]; then
    echo "cargo was not found; starting the existing build."
else
    echo "cargo was not found and there is no built sandbox. Install Rust from https://rustup.rs (the toolchain in rust-toolchain.toml is fetched automatically)."
    exit 1
fi

export BEVY_ASSET_ROOT="$PWD/Sandbox"
exec "$BINARY"
