#!/usr/bin/env bash
set -euo pipefail

cargo test --test m003_runtime_integration
cargo test
bash scripts/verify-m002.sh

echo "verify-m003: integrated runtime checks passed"
