#!/usr/bin/env bash
# Full check. Fails loudly rather than reporting a total that hides failures —
# an earlier version summed the "passed" column across suites and cheerfully
# printed a healthy number while three tests were red.
set -euo pipefail

cd "$(dirname "$0")"

echo "== fmt =="
cargo fmt --all --check

echo "== clippy =="
cargo clippy --all-targets --all-features -- -D warnings

# The network feature is not exercised by the test suite, so without this it
# could rot unnoticed until someone tries to ship.
echo "== network feature builds =="
cargo build -p vardo-drive --features network --quiet

echo "== tests =="
output=$(cargo test --all 2>&1) || {
	echo "$output"
	exit 1
}
echo "$output" | grep -E "^test result" || true

if echo "$output" | grep -qE "FAILED|panicked"; then
	echo "$output"
	echo "FAILURES PRESENT"
	exit 1
fi

passed=$(echo "$output" | grep -oE "^test result: ok\. [0-9]+" | grep -oE "[0-9]+" | awk '{ s += $1 } END { print s }')
echo "OK — $passed tests passed"

# --- frontend -------------------------------------------------------------
# Skipped rather than failed when dependencies are absent, so a Rust-only
# checkout still runs green.
if [ -d app/node_modules ]; then
	echo
	echo "== app: typecheck =="
	(cd app && npx tsc --noEmit)

	# The frontend had no tests at all for most of this project's life, and
	# every UI bug in it was found by a person looking at the screen. These
	# cover the pure logic that actually shipped wrong: ages rendering as "0mo",
	# avatar colours splitting one site into two, an initial cutting a surrogate
	# pair in half, and layout thresholds that let three panes run off the edge
	# of a narrow window.
	echo "== app: tests =="
	(cd app && npx vitest run --reporter dot)

	# The ported Telegram palette is not APCA-verified upstream, and several
	# pairs fail WCAG AA as shipped. This gate is why the deviations in
	# tokens.css are measured rather than asserted — it caught nine failures
	# that eyeballing missed, mostly hover states and the focus ring.
	echo "== app: contrast =="
	(cd app && node scripts/contrast.mjs)

	echo "== app: build =="
	(cd app && npx vite build >/dev/null)
	echo "OK — app builds"
else
	echo
	echo "skipping app checks (run: cd app && npm install)"
fi
