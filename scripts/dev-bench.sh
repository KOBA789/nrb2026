#!/usr/bin/env bash
# webapp が 127.0.0.1:8080 で listen している前提で bench を 1 発回す。
#
# webapp の起動はユーザ責任 (`cd webapp && cargo run`、または scripts/e2e.sh 経由)。
# bench は release ビルドで実行する (本番 AMI と同じ)。
#
# env vars は scripts/test-local.sh と同じ慣習にして VM 経路と差を出さない。
# 仕様: docs/authoring/dev-loop.md
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mkdir -p "$ROOT/build"
REPORT="$ROOT/build/dev-report.json"

echo "==> cargo build --release -p bench"
# nrb2026 root は workspace (members = bench/seed-data/seed-gen)。bench の
# 成果物は $ROOT/target/release/bench に入る (旧 bench/target/ ではない)。
( cd "$ROOT" && cargo build --release -p bench )

echo "==> ensure bench fixtures (idempotent)"
"$ROOT/scripts/fetch-bench-fixtures.sh"

echo "==> run bench (REPORT_FD=3 -> ${REPORT})"
rm -f "$REPORT"
# AMI 側の /opt/bench/bench.sh と同じ慣習で BENCH_FIXTURES_DIR を渡す
# (向き先だけ local repo に差し替え)。
REPORT_FD=3 \
BENCHWARMER_target_index=0 \
BENCHWARMER_target_ip=127.0.0.1 \
BENCHWARMER_all_ips=127.0.0.1 \
WEBHOOK_URL=http://127.0.0.1:9999/webhook \
BENCH_FIXTURES_DIR="$ROOT/bench/fixtures/images" \
    "$ROOT/target/release/bench" 3>"$REPORT"

cat "$REPORT"

SCORE="$(jq -r .score < "$REPORT")"
echo "score=${SCORE}"
if [[ -z "${SCORE}" ]] || [[ "${SCORE}" -lt 1 ]]; then
    echo "FAIL: bench reported score < 1 (or unparsable)" >&2
    exit 1
fi
echo "OK: bench reported score=${SCORE}"
