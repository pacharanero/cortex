#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exercise s/linkcheck in isolated temporary Git repositories.

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
	mkdir -p "$repo/docs"
	git -C "$repo" init -q
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
write_page a.md 'See [b](b.md).'
write_page b.md '# B'
expect_success "relative link to an existing page"

reset_case
write_page a.md 'See [gone](missing.md).'
expect_failure "link to a missing page" "does not resolve to a file"

reset_case
write_page a.md $'# A\n\nJump to [section](#a-heading).\n\n## A heading'
expect_success "same-file anchor matches an auto-slugged heading"

reset_case
write_page a.md '[nowhere](#does-not-exist)'
write_page b.md '# B'
expect_failure "same-file anchor with no matching heading" "no heading in"

reset_case
write_page a.md $'[jump](b.md#custom-id)'
write_page b.md $'# B\n\n## Real heading {#custom-id}'
expect_success "cross-file anchor matches an explicit attr_list id"

reset_case
write_page a.md '[external](https://example.com/should-not-be-checked)'
expect_success "external links are skipped entirely"

reset_case
write_page a.md $'```text\n[fenced](not-real.md)\n```\nReal text.'
expect_success "a link inside a fenced code block is not checked"

if [[ "$failures" -gt 0 ]]; then
	echo "ERROR: $failures of $checks linkcheck tests failed." >&2
	exit 1
fi

echo "linkcheck tests ok: $checks cases"
