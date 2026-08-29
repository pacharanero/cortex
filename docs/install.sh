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
MIN_GLIBC='2.34'

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

glibc_is_supported() {
    version="$1"
    major="${version%%.*}"
    minor="${version#*.}"
    minor="${minor%%.*}"
    case "$major" in ''|*[!0-9]*) return 1 ;; esac
    case "$minor" in ''|*[!0-9]*) return 1 ;; esac
    [ "$major" -gt 2 ] || { [ "$major" -eq 2 ] && [ "$minor" -ge 34 ]; }
}

check_runtime() {
    stage="$1"
    command -v getconf >/dev/null 2>&1 || err "cannot verify glibc $MIN_GLIBC compatibility: getconf is missing"
    glibc="$(getconf GNU_LIBC_VERSION 2>/dev/null | sed -n 's/^glibc[[:space:]]*//p')"
    [ -n "$glibc" ] || err "released binaries require glibc $MIN_GLIBC or newer; this system does not report glibc"
    glibc_is_supported "$glibc" || err "released binaries require glibc $MIN_GLIBC or newer; this system reports $glibc"

    command -v ldd >/dev/null 2>&1 || err 'cannot verify runtime libraries: ldd is missing'
    for binary in "$stage/cortex" "$stage/cortex-mcp"; do
        dependencies="$(ldd "$binary" 2>&1)" || err "could not inspect runtime libraries for ${binary##*/}: $dependencies"
        if printf '%s\n' "$dependencies" | grep -q 'libudev\.so\.1 => not found'; then
            err 'libudev.so.1 is required; install libudev (often packaged as libudev1 or systemd-libs)'
        fi
        if printf '%s\n' "$dependencies" | grep -q '=> not found'; then
            err "a runtime library required by ${binary##*/} is missing: $dependencies"
        fi
    done

    "$stage/cortex" --version >/dev/null 2>&1 || err 'downloaded cortex binary cannot run on this system'
    mcp_stdout="$stage/.cortex-mcp-probe.out"
    mcp_stderr="$stage/.cortex-mcp-probe.err"
    mcp_initialize='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"installer-probe","version":"1"}}}'
    if ! printf '%s\n' "$mcp_initialize" | "$stage/cortex-mcp" >"$mcp_stdout" 2>"$mcp_stderr"; then
        rm -f "$mcp_stdout" "$mcp_stderr"
        err 'downloaded cortex-mcp binary cannot complete MCP initialization on this system'
    fi
    if ! grep -Fq '"jsonrpc":"2.0"' "$mcp_stdout" ||
       ! grep -Fq '"protocolVersion":"2025-11-25"' "$mcp_stdout" ||
       ! grep -Fq '"name":"cortex-mcp"' "$mcp_stdout"; then
        rm -f "$mcp_stdout" "$mcp_stderr"
        err 'downloaded cortex-mcp binary returned an invalid MCP initialization response'
    fi
    rm -f "$mcp_stdout" "$mcp_stderr"
}

install_release() {
    stage="$1"
    cli_new="$INSTALL_DIR/.cortex.new.$$"
    mcp_new="$INSTALL_DIR/.cortex-mcp.new.$$"
    rule_new="$INSTALL_DIR/.70-neural-dsp-cortex.rules.new.$$"
    cli_old="$INSTALL_DIR/.cortex.old.$$"
    mcp_old="$INSTALL_DIR/.cortex-mcp.old.$$"
    rule_old="$INSTALL_DIR/.70-neural-dsp-cortex.rules.old.$$"

    rm -f "$cli_new" "$mcp_new" "$rule_new" "$cli_old" "$mcp_old" "$rule_old"
    if ! install -m 0755 "$stage/cortex" "$cli_new" ||
       ! install -m 0755 "$stage/cortex-mcp" "$mcp_new" ||
       ! install -m 0644 "$stage/70-neural-dsp-cortex.rules" "$rule_new"; then
        rm -f "$cli_new" "$mcp_new" "$rule_new"
        err 'could not stage the release in the install directory; the existing installation was not changed'
    fi

    had_cli=false
    had_mcp=false
    had_rule=false
    backup_failed=false
    if [ -e "$INSTALL_DIR/cortex" ]; then
        had_cli=true
        cp -p "$INSTALL_DIR/cortex" "$cli_old" || backup_failed=true
    fi
    if [ -e "$INSTALL_DIR/cortex-mcp" ]; then
        had_mcp=true
        cp -p "$INSTALL_DIR/cortex-mcp" "$mcp_old" || backup_failed=true
    fi
    if [ -e "$INSTALL_DIR/70-neural-dsp-cortex.rules" ]; then
        had_rule=true
        cp -p "$INSTALL_DIR/70-neural-dsp-cortex.rules" "$rule_old" || backup_failed=true
    fi
    if [ "$backup_failed" = true ]; then
        rm -f "$cli_new" "$mcp_new" "$rule_new" "$cli_old" "$mcp_old" "$rule_old"
        err 'could not back up the installed release; the existing installation was not changed'
    fi

    if mv "$cli_new" "$INSTALL_DIR/cortex" &&
       mv "$mcp_new" "$INSTALL_DIR/cortex-mcp" &&
       mv "$rule_new" "$INSTALL_DIR/70-neural-dsp-cortex.rules"; then
        rm -f "$cli_old" "$mcp_old" "$rule_old" || true
        return
    fi

    rollback_failed=false
    if [ "$had_cli" = true ]; then
        mv "$cli_old" "$INSTALL_DIR/cortex" || rollback_failed=true
    else
        rm -f "$INSTALL_DIR/cortex" || rollback_failed=true
    fi
    if [ "$had_mcp" = true ]; then
        mv "$mcp_old" "$INSTALL_DIR/cortex-mcp" || rollback_failed=true
    else
        rm -f "$INSTALL_DIR/cortex-mcp" || rollback_failed=true
    fi
    if [ "$had_rule" = true ]; then
        mv "$rule_old" "$INSTALL_DIR/70-neural-dsp-cortex.rules" || rollback_failed=true
    else
        rm -f "$INSTALL_DIR/70-neural-dsp-cortex.rules" || rollback_failed=true
    fi
    rm -f "$cli_new" "$mcp_new" "$rule_new"
    [ "$rollback_failed" = false ] || err 'could not replace or fully restore the installed release; inspect the install directory'
    err 'could not replace the installed release; the previous installation was restored'
}

main() {
    [ "$(uname -s)" = Linux ] || err 'only Linux is supported by the released host binaries'
    case "$(uname -m)" in
        x86_64|amd64) ;;
        *) err "unsupported Linux architecture: $(uname -m); only x86_64 is released" ;;
    esac
    command -v tar >/dev/null 2>&1 || err 'install tar first'
    command -v xz >/dev/null 2>&1 || err 'install xz first (the package is usually named xz-utils or xz)'
    command -v install >/dev/null 2>&1 || err 'install the coreutils install command first'

    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM
    version="${CORTEX_VERSION:-}"
    if [ -z "$version" ]; then
        info 'Looking up the latest cortex release...'
        version="$(latest_version "$tmpdir/release.json")"
        [ -n "$version" ] || err 'could not determine the latest release'
    fi
    printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$' || err "invalid release version: $version"

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
    [ -f "$stage/70-neural-dsp-cortex.rules" ] || err 'archive did not contain the udev rule'
    check_runtime "$stage"
    info "Runtime compatibility OK (glibc $glibc; shared libraries present)."
    mkdir -p "$INSTALL_DIR"
    install_release "$stage"

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
