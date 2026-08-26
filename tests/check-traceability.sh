#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exercise s/check-traceability in isolated temporary Git repositories.

set -euo pipefail
root="$(git rev-parse --show-toplevel)"
checker="$root/s/check-traceability"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

repo="$tmpdir/repo"
failures=0
checks=0

reset_case() {
	rm -rf "$repo"
	mkdir -p "$repo/src" "$repo/spec/100-test"
	git -C "$repo" init -q
}

write_source() {
	printf '%s\n' "$1" > "$repo/src/lib.rs"
}

write_target() {
	local path="$1"
	local content="$2"
	mkdir -p "$(dirname "$repo/$path")"
	printf '%s\n' "$content" > "$repo/$path"
}

stage_case() {
	git -C "$repo" add .
}

pass() {
	checks=$((checks + 1))
}

fail() {
	local name="$1"
	local output="$2"
	checks=$((checks + 1))
	failures=$((failures + 1))
	printf 'FAIL: %s\n%s\n' "$name" "$output" >&2
}

expect_success() {
	local name="$1"
	local output
	if output="$(cd "$repo" && "$checker" 2>&1)"; then
		pass
	else
		fail "$name" "$output"
	fi
}

expect_failure() {
	local name="$1"
	local expected="$2"
	local output
	if output="$(cd "$repo" && "$checker" 2>&1)"; then
		fail "$name" "expected failure, got success: $output"
	elif [[ "$output" != *"$expected"* ]]; then
		fail "$name" "expected '$expected' in output: $output"
	else
		pass
	fi
}

reset_case
write_source '//! @see spec/100-test/spec.md [ROAD.1] [DES.TEST] [FR.1]'
write_target spec/100-test/spec.md $'- [x] **ROAD.1**: complete\n## [DES.TEST] Design\n| FR.1 | Requirement |'
stage_case
expect_success "all supported node forms resolve literally"

reset_case
write_source '//! @see spec/100-test/design.md'
write_target spec/100-test/design.md '# Design'
stage_case
expect_success "path-only living target"

reset_case
write_source '//! @see spec/100-test/spec.md ROAD-1'
write_target spec/100-test/spec.md '- [~] **ROAD-1**: partial'
stage_case
expect_success "lone unbracketed id"

reset_case
write_source '//! no traceability header yet'
stage_case
expect_failure "missing header" "missing @see traceability header"

reset_case
write_source '// @see spec/100-test/spec.md'
write_target spec/100-test/spec.md '# Spec'
stage_case
expect_failure "ordinary comment is not a header" "missing @see traceability header"

reset_case
write_source '    //! @see spec/100-test/spec.md'
write_target spec/100-test/spec.md '# Spec'
stage_case
expect_failure "nested doc comment is not a top-level header" "missing @see traceability header"

reset_case
write_source '//! @see spec/100-test/missing.md'
stage_case
expect_failure "missing target" "does not exist"

reset_case
write_source '//! @see spec/100-test/spec.md [FR-2]'
write_target spec/100-test/spec.md '| FR-1 | Requirement |'
stage_case
expect_failure "missing node" "node id not found"

reset_case
write_source '//! @see spec/100-test/spec.md [FR-1'
write_target spec/100-test/spec.md '| FR-1 | Requirement |'
stage_case
expect_failure "unclosed bracket" "malformed @see"

reset_case
write_source '//! @see spec/100-test/spec.md [FR-1] trailing'
write_target spec/100-test/spec.md '| FR-1 | Requirement |'
stage_case
expect_failure "trailing text" "malformed @see"

reset_case
write_source '//! @see'
stage_case
expect_failure "missing target syntax" "missing a target"

reset_case
write_source '//! @see spec/100-test/design.md [DES.TEST]'
write_target spec/100-test/design.md '## [DESXTEST] Wrong literal id'
stage_case
expect_failure "regex metacharacters are literal" "node id not found"

reset_case
write_source '//! @see spec/100-test/spec.md [FR-1]'
write_target spec/100-test/spec.md 'Ordinary prose mentions **FR-1** but defines no node.'
stage_case
expect_failure "prose mention is not a node" "node id not found"

reset_case
write_source '//! @see spec/roadmap.md ROAD-1'
write_target spec/roadmap.md '- [x] **ROAD-1**: complete'
stage_case
expect_failure "progress ledger alone is insufficient" "must include at least one"

if [[ "$failures" -gt 0 ]]; then
	echo "ERROR: $failures of $checks traceability checker tests failed." >&2
	exit 1
fi

echo "traceability checker tests ok: $checks cases"
