#!/bin/sh
# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Download the released Linux x86_64 archive, verify its published SHA-256
# checksum, and install both host binaries.
#
# Usage:
#   curl -LsSf https://pacharanero.github.io/cortex/install.sh | sh
#
# Environment:
#   CORTEX_INSTALL_DIR  destination directory (default: ~/.local/bin)
#   CORTEX_VERSION      release tag to install, such as v0.1.0 (default: latest)

set -eu

REPO='pacharanero/cortex'
TARGET='x86_64-unknown-linux-gnu'
INSTALL_DIR="${CORTEX_INSTALL_DIR:-$HOME/.local/bin}"

err() { printf 'error: %s\n' "$*" >&2; exit 1; }
info() { printf '%s\n' "$*"; }

fetch() {
    url="$1"
    output="$2"
    if command -v curl >/dev/null 2>&1; then
        curl --fail --location --proto '=https' --tlsv1.2 --output "$output" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$output" "$url"
    else
        err 'install curl or wget first'
    fi
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        err 'install sha256sum or shasum first'
    fi
}

latest_version() {
    fetch "https://api.github.com/repos/$REPO/releases/latest" "$1"
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | sed -n '1p'
}

main() {
    [ "$(uname -s)" = Linux ] || err 'only Linux is supported by the released host binaries'
    case "$(uname -m)" in
        x86_64|amd64) ;;
        *) err "unsupported Linux architecture: $(uname -m); only x86_64 is released" ;;
    esac

    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM
    version="${CORTEX_VERSION:-}"
    if [ -z "$version" ]; then
        info 'Looking up the latest cortex release...'
        version="$(latest_version "$tmpdir/release.json")"
        [ -n "$version" ] || err 'could not determine the latest release'
    fi

    archive="cortex-${version#v}-${TARGET}.tar.xz"
    base="https://github.com/$REPO/releases/download/$version"
    info "Downloading cortex $version for $TARGET..."
    fetch "$base/$archive" "$tmpdir/$archive"
    fetch "$base/SHA256SUMS" "$tmpdir/SHA256SUMS"
    expected="$(sed -n "s/^[[:space:]]*\([0-9A-Fa-f][0-9A-Fa-f]*\)[[:space:]][[:space:]]*\*\{0,1\}${archive}$/\1/p" "$tmpdir/SHA256SUMS")"
    [ -n "$expected" ] || err "no checksum for $archive in SHA256SUMS"
    actual="$(sha256_of "$tmpdir/$archive")"
    [ "$expected" = "$actual" ] || err "checksum mismatch for $archive"
    info 'Checksum OK.'

    tar -xJf "$tmpdir/$archive" -C "$tmpdir"
    stage="$tmpdir/cortex-${version#v}-${TARGET}"
    [ -x "$stage/cortex" ] || err 'archive did not contain cortex'
    [ -x "$stage/cortex-mcp" ] || err 'archive did not contain cortex-mcp'
    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$stage/cortex" "$INSTALL_DIR/cortex"
    install -m 0755 "$stage/cortex-mcp" "$INSTALL_DIR/cortex-mcp"
    install -m 0644 "$stage/70-neural-dsp-cortex.rules" "$INSTALL_DIR/70-neural-dsp-cortex.rules"

    info "Installed cortex and cortex-mcp to $INSTALL_DIR"
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) info "$INSTALL_DIR is not on PATH. Add it in your shell startup file before running cortex." ;;
    esac
    "$INSTALL_DIR/cortex" completions install 2>/dev/null || \
        info 'Run `cortex completions install` after choosing or starting a supported shell.'
    info 'Next: run `cortex setup` to check device access and print the one-off udev and MCP steps.'
}

main "$@"
