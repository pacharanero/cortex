#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
version="v9.8.7"
target="x86_64-unknown-linux-gnu"
release="$tmp/release"
stage_root="$tmp/stage"
stage="$stage_root/cortex-${version#v}-${target}"
fake_bin="$tmp/bin"
install_dir="$tmp/install"
mkdir -p "$release" "$stage" "$fake_bin" "$install_dir"

printf '%s\n' '#!/bin/sh' 'if [ "${1:-}" = "--version" ]; then echo "cortex 9.8.7"; fi' 'exit 0' > "$stage/cortex"
cat > "$stage/cortex-mcp" <<'SH'
#!/bin/sh
[ "${FAKE_MCP_START_FAILURE:-}" != 1 ] || exit 1
IFS= read -r request
case "$request" in
    *'"method":"initialize"'*) ;;
    *) exit 1 ;;
esac
if [ "${FAKE_MCP_BAD_RESPONSE:-}" = 1 ]; then
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","serverInfo":{"name":"not-cortex-mcp"}}}'
else
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","serverInfo":{"name":"cortex-mcp"}}}'
fi
SH
printf '%s\n' 'fictional rule' > "$stage/70-neural-dsp-cortex.rules"
chmod +x "$stage/cortex" "$stage/cortex-mcp"
archive="cortex-${version#v}-${target}.tar.xz"
tar -C "$stage_root" -cJf "$release/$archive" "${stage##*/}"
(cd "$release" && sha256sum "$archive" > SHA256SUMS)

cat > "$fake_bin/curl" <<'SH'
#!/bin/sh
set -eu
output=''
url=''
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) shift; output="$1" ;;
        https://*) url="$1" ;;
    esac
    shift
done
cp "$FIXTURE_RELEASE_DIR/${url##*/}" "$output"
SH
cat > "$fake_bin/uname" <<'SH'
#!/bin/sh
case "$1" in
    -s) printf '%s\n' Linux ;;
    -m) printf '%s\n' x86_64 ;;
    *) exit 1 ;;
esac
SH
cat > "$fake_bin/getconf" <<'SH'
#!/bin/sh
[ "$1" = GNU_LIBC_VERSION ] || exit 1
printf 'glibc %s\n' "${FAKE_GLIBC_VERSION:-2.34}"
SH
cat > "$fake_bin/ldd" <<'SH'
#!/bin/sh
case "${FAKE_RUNTIME_FAILURE:-}" in
    udev)
        printf '%s\n' 'libudev.so.1 => not found'
        ;;
    mcp)
        case "$1" in
            *cortex-mcp) printf '%s\n' 'libfictional.so.1 => not found' ;;
            *) printf '%s\n' 'libudev.so.1 => /usr/lib/libudev.so.1' ;;
        esac
        ;;
    *)
        printf '%s\n' 'libudev.so.1 => /usr/lib/libudev.so.1'
        ;;
esac
SH
real_mv="$(command -v mv)"
cat > "$fake_bin/mv" <<'SH'
#!/bin/sh
set -eu
if [ "${FAKE_MV_FAILURE:-}" = mcp ] &&
   [ "${2:-}" = "$CORTEX_INSTALL_DIR/cortex-mcp" ] &&
   [ ! -e "$FAKE_MV_MARKER" ]; then
    : > "$FAKE_MV_MARKER"
    exit 1
fi
exec "$REAL_MV" "$@"
SH
chmod +x "$fake_bin/curl" "$fake_bin/uname" "$fake_bin/getconf" "$fake_bin/ldd" "$fake_bin/mv"

run_installer() {
    env \
        PATH="$fake_bin:$PATH" \
        FIXTURE_RELEASE_DIR="$release" \
        FAKE_GLIBC_VERSION="${FAKE_GLIBC_VERSION:-2.34}" \
        FAKE_RUNTIME_FAILURE="${FAKE_RUNTIME_FAILURE:-}" \
        FAKE_MCP_START_FAILURE="${FAKE_MCP_START_FAILURE:-}" \
        FAKE_MCP_BAD_RESPONSE="${FAKE_MCP_BAD_RESPONSE:-}" \
        FAKE_MV_FAILURE="${FAKE_MV_FAILURE:-}" \
        FAKE_MV_MARKER="$tmp/mv-failed" \
        REAL_MV="$real_mv" \
        CORTEX_VERSION="$version" \
        CORTEX_INSTALL_DIR="$install_dir" \
        sh docs/install.sh
}

printf '%s\n' old-cortex > "$install_dir/cortex"
printf '%s\n' old-mcp > "$install_dir/cortex-mcp"
run_installer >/dev/null
cmp "$stage/cortex" "$install_dir/cortex"
cmp "$stage/cortex-mcp" "$install_dir/cortex-mcp"

assert_refused_without_replacement() {
    printf '%s\n' old-cortex > "$install_dir/cortex"
    printf '%s\n' old-mcp > "$install_dir/cortex-mcp"
    if run_installer >"$tmp/refusal.out" 2>"$tmp/refusal.err"; then
        echo "installer unexpectedly accepted an incompatible runtime" >&2
        exit 1
    fi
    grep -qx old-cortex "$install_dir/cortex"
    grep -qx old-mcp "$install_dir/cortex-mcp"
}

FAKE_GLIBC_VERSION=2.33 assert_refused_without_replacement
FAKE_GLIBC_VERSION=2.34 FAKE_RUNTIME_FAILURE=udev assert_refused_without_replacement
FAKE_GLIBC_VERSION=2.34 FAKE_RUNTIME_FAILURE=mcp assert_refused_without_replacement
FAKE_GLIBC_VERSION=2.34 FAKE_MCP_START_FAILURE=1 assert_refused_without_replacement
FAKE_GLIBC_VERSION=2.34 FAKE_MCP_BAD_RESPONSE=1 assert_refused_without_replacement
rm -f "$tmp/mv-failed"
FAKE_GLIBC_VERSION=2.34 FAKE_MV_FAILURE=mcp assert_refused_without_replacement

printf 'released installer fixtures ok\n'
