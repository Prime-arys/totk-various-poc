#!/usr/bin/env bash
# The tests of the crates that also run on a PC. The plugins themselves only
# build for the console (the skyline crate), so they are left out.

set -e
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$ROOT"
exec cargo test -p totk-formats -p totk-merge "$@"
