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
