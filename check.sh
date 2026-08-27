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

passed=$(echo "$output" | grep -oE "^test result: ok\. [0-9]+" | grep -oE "[0-9]+" | paste -sd+ | bc)
echo "OK — $passed tests passed"

# --- frontend -------------------------------------------------------------
# Skipped rather than failed when dependencies are absent, so a Rust-only
# checkout still runs green.
if [ -d app/node_modules ]; then
	echo
	echo "== app: typecheck =="
	(cd app && npx tsc --noEmit)

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
