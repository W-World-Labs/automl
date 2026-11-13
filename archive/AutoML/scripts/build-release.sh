#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$ROOT/automl-web"
npm ci
npm run build

cd "$ROOT"
cargo test
cargo build --release -p automl-server -p automl-cli
