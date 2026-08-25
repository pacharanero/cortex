#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exercise rendered-site link validation in isolated temporary repositories.

set -euo pipefail
root="$(git rev-parse --show-toplevel)"
checker="$root/s/linkcheck"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

repo="$tmpdir/repo"
failures=0
checks=0

reset_case() {
    rm -rf "$repo"
    mkdir -p "$repo/docs" "$repo/site"
    git -C "$repo" init -q
}

write_html() {
    local path="$1" content="$2"
    mkdir -p "$(dirname "$repo/site/$path")"
    printf '%s\n' "$content" > "$repo/site/$path"
}

write_page() {
    local path="$1" content="$2"
    mkdir -p "$(dirname "$repo/docs/$path")"
    printf '%s\n' "$content" > "$repo/docs/$path"
}

pass() { checks=$((checks + 1)); }

fail() {
    local name="$1" output="$2"
    checks=$((checks + 1))
    failures=$((failures + 1))
    printf 'FAIL: %s\n%s\n' "$name" "$output" >&2
}

expect_site_success() {
    local name="$1" output
    if output="$(cd "$repo" && CORTEX_LINKCHECK_SITE=site "$checker" 2>&1)"; then
        pass
    else
        fail "$name" "$output"
    fi
}

expect_site_failure() {
    local name="$1" expected="$2" output
    if output="$(cd "$repo" && CORTEX_LINKCHECK_SITE=site "$checker" 2>&1)"; then
        fail "$name" "expected failure, got success: $output"
    elif [[ "$output" != *"$expected"* ]]; then
        fail "$name" "expected '$expected' in output: $output"
    else
        pass
    fi
}

reset_case
write_html index.html '<article><h1 id="home">Home</h1><a href="guide/#caf%C3%A9">Guide</a><a href="https://example.com/">External</a></article>'
write_html guide/index.html '<article><h1 id="café">Café</h1><h2 id="repeat_1">Repeat</h2></article>'
expect_site_success "relative rendered links, encoded Unicode anchors and external links"

reset_case
write_html index.html '<article><a href="missing/">Missing</a></article>'
expect_site_failure "missing rendered target" "does not resolve inside the built site"

reset_case
write_html index.html '<article><a href="#missing">Missing anchor</a></article>'
expect_site_failure "missing rendered anchor" "has no rendered target anchor"

reset_case
write_html index.html '<article><a href="../README.md">Outside</a></article>'
expect_site_failure "target outside deployed documentation" "escapes the built documentation site"

reset_case
cat > "$repo/mkdocs.yml" <<'EOF'
site_url: https://example.test/cortex/
EOF
write_html index.html '<article><a href="/install/">Wrong deployment root</a></article>'
write_html install/index.html '<article><h1>Install</h1></article>'
expect_site_failure "root-relative target must include deployed prefix" "omits deployed site prefix '/cortex'"

reset_case
write_html index.html '<article><img src="missing.png" alt="Missing"></article>'
expect_site_failure "missing rendered image" "does not resolve inside the built site"

reset_case
if output="$(cd "$repo" && CORTEX_LINKCHECK_SITE=absent "$checker" 2>&1)"; then
    fail "missing rendered site fails closed" "expected failure, got success: $output"
elif [[ "$output" != *"rendered site directory does not exist"* ]]; then
    fail "missing rendered site fails closed" "unexpected output: $output"
else
    pass
fi

reset_case
if output="$(cd "$repo" && CORTEX_LINKCHECK_SITE=site "$checker" 2>&1)"; then
    fail "empty rendered site fails closed" "expected failure, got success: $output"
elif [[ "$output" != *"rendered site contains no HTML documents"* ]]; then
    fail "empty rendered site fails closed" "unexpected output: $output"
else
    pass
fi

reset_case
cat > "$repo/mkdocs.yml" <<'EOF'
site_name: fixture
nav:
  - Home: index.md
  - Other: other.md
EOF
write_page index.md $'# Home\n\n[link with title](other.md "Title")\n\n[reference link][other]\n\n`[inline code](missing.md)`\n\n## Café\n\n[Unicode](#cafe)\n\n## Repeat\n\n## Repeat\n\n[Duplicate](#repeat_1)\n\n## Label {#custom-id}\n\n[Explicit](#custom-id)\n\n[other]: other.md'
write_page other.md '# Other'
if output="$(cd "$repo" && "$checker" 2>&1)"; then
    pass
else
    fail "Zensical determines Markdown links and heading IDs" "$output"
fi

if [[ "$failures" -gt 0 ]]; then
    echo "ERROR: $failures of $checks linkcheck tests failed." >&2
    exit 1
fi

echo "linkcheck tests ok: $checks cases"
