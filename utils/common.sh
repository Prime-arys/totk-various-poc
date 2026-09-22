# Shared by the build scripts in utils/. Sourced, not run.
#
# Everything here assumes the repository's container (docs/build.md): a Linux
# with devkitPro in /opt/devkitpro and the Rust toolchains in /opt/rust. That
# is the only supported way to build, which is why there is nothing here about
# picking a shell, converting a path or finding a writable temporary folder —
# all of which the Windows host used to need.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Two directories, and only two: what the build writes while it works, and
# what it leaves for you. Both are ignored by git.
OUTPUT="${TOTK_OUTPUT:-$ROOT/output}"
RELEASE="${TOTK_RELEASE:-$ROOT/release}"

TITLE_ID="${TITLE_ID:-0100F2C0115B6000}"

# cargo, and anything that needs a scratch file, write inside output/ as well.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$OUTPUT/cargo}"
export TMPDIR="${TMPDIR:-$OUTPUT/tmp}"
mkdir -p "$CARGO_TARGET_DIR" "$TMPDIR"

say() {
    printf '\033[1m%s\033[0m\n' "$*"
}
