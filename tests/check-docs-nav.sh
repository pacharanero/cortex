#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exercise s/check-docs-nav in isolated temporary Git repositories.

set -euo pipefail
root="$(git rev-parse --show-toplevel)"
checker="$root/s/check-docs-nav"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

repo="$tmpdir/repo"
failures=0
checks=0

reset_case() {
	rm -rf "$repo"
	mkdir -p "$repo/docs"
	git -C "$repo" init -q
}

write_mkdocs() {
	printf '%s\n' "$1" > "$repo/mkdocs.yml"
}

write_page() {
	local path="$1"
	mkdir -p "$(dirname "$repo/docs/$path")"
	printf '# %s\n' "$path" > "$repo/docs/$path"
}

write_exceptions() {
	printf '%s\n' "$1" > "$repo/.nav-exceptions"
}

pass() { checks=$((checks + 1)); }

fail() {
	local name="$1" output="$2"
	checks=$((checks + 1))
	failures=$((failures + 1))
	printf 'FAIL: %s\n%s\n' "$name" "$output" >&2
}

expect_success() {
	local name="$1" output
	if output="$(cd "$repo" && "$checker" 2>&1)"; then
		pass
	else
		fail "$name" "$output"
	fi
}

expect_failure() {
	local name="$1" expected="$2" output
	if output="$(cd "$repo" && "$checker" 2>&1)"; then
		fail "$name" "expected failure, got success: $output"
	elif [[ "$output" != *"$expected"* ]]; then
		fail "$name" "expected '$expected' in output: $output"
	else
		pass
	fi
}

reset_case
write_mkdocs $'nav:\n  - Home: index.md\n  - Install: install.md'
write_page index.md
write_page install.md
expect_success "every page referenced in nav"

reset_case
write_mkdocs $'site_description: mention.md outside nav\nnav:\n  - Home: index.md'
write_page index.md
expect_success "Markdown-like config text outside nav is ignored"

reset_case
write_mkdocs $'nav:\n  - Home: index.md'
write_page index.md
write_page orphan.md
expect_failure "unreferenced page is an orphan" "orphan.md: not referenced"

reset_case
write_mkdocs $'nav:\n  - Home: index.md\n  - Missing: missing.md'
write_page index.md
expect_failure "nav entry naming a missing page fails" "nav entry 'missing.md' does not exist"

reset_case
write_mkdocs $'nav:\n  - Home: index.md\n  - Sub:\n      - Overview: sub/overview.md'
write_page index.md
write_page sub/overview.md
expect_success "nested nav section is still discovered"

reset_case
write_mkdocs $'nav:\n  - Home: index.md\n  # - Removed: missing.md'
write_page index.md
expect_success "commented nav entry is ignored"

reset_case
write_mkdocs $'nav:\n  - Home: index.md'
write_page index.md
write_page draft.md
write_exceptions "docs/draft.md	work in progress, not ready to publish"
expect_success "named exception with a reason suppresses the orphan"

reset_case
write_mkdocs $'nav:\n  - Home: index.md'
write_page index.md
write_exceptions "docs/gone.md	page was removed but exception was not"
expect_failure "stale exception naming a deleted page fails" "no longer exists"

reset_case
write_mkdocs $'nav:\n  - Home: index.md'
write_page index.md
write_exceptions "docs/nowhere.md"
expect_failure "exception line with no reason is malformed" "expected '<path><TAB><reason>'"

if [[ "$failures" -gt 0 ]]; then
	echo "ERROR: $failures of $checks nav-orphan checker tests failed." >&2
	exit 1
fi

echo "nav-orphan checker tests ok: $checks cases"
