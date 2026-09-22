#!/usr/bin/env bash
# Copies finished files into release/, which is the only place the build
# leaves things to be used elsewhere.
#
#   utils/release-copy.sh --out <dir> <file>...

set -e
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

OUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --out) shift; OUT="$1" ;;
        *) break ;;
    esac
    shift
done
if [ -z "$OUT" ] || [ $# -eq 0 ]; then
    echo "usage: utils/release-copy.sh --out <dir> <file>..." >&2
    exit 2
fi

mkdir -p "$OUT"
for file in "$@"; do
    cp "$file" "$OUT/"
    say "Copied $(basename "$file") to $OUT/"
done
