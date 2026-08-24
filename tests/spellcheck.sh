#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exercise s/spellcheck in isolated temporary Git repositories. Runs the
# real codespell (installing the pinned version on first use, same as a
# normal invocation), so this needs network access the first time it runs
# in a fresh environment - exactly like the script it is testing.

set -euo pipefail
root="$(git rev-parse --show-toplevel)"
checker="$root/s/spellcheck"
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
write_page clean.md '# Clean page

Nothing here is misspelled.'
expect_success "clean prose passes"

reset_case
write_page typo.md '# Typo page

We shoud not recieve a false pass on an obvious typo.'
expect_failure "an obvious typo fails" "shoud"

if [[ "$failures" -gt 0 ]]; then
	echo "ERROR: $failures of $checks spellcheck tests failed." >&2
	exit 1
fi

echo "spellcheck tests ok: $checks cases"
