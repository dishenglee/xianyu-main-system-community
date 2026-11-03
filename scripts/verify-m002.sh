#!/usr/bin/env bash
set -euo pipefail

cargo test
cargo test --test fixture_parity
bash scripts/verify-s01.sh

echo "verify-m002: compatibility spine checks passed"
