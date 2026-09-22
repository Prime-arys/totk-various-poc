#!/usr/bin/env bash
# Build helper: sets devkitPro up and runs make.
# Usage: ./build.sh [make args...]
#
# The repository calls it from its container, with the folder to build into:
#   ./build.sh package OUT_ROOT=/work/output/skyline
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export DEVKITPRO=/opt/devkitpro
export DEVKITA64=$DEVKITPRO/devkitA64
export PATH="$DEVKITA64/bin:$DEVKITPRO/tools/bin:$PATH"
cd "$DIR"
exec make "$@"
