#!/usr/bin/env bash
# ローカル e2e ワンショット: compose の MySQL → webapp (debug) → bench を 1 サイクル回す。
#
# - compose は up したまま残す (毎回立て直すと slow)。明示的に止めたい時は
#   `docker compose down` (volume も消すなら `down -v`)。
# - webapp は debug build で bg 起動し、trap で必ず kill する。
# - スコア < 1 で fail、スコア >= 1 で OK。
#
# 仕様: docs/authoring/dev-loop.md
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# 配布版生成物 (gitignored) を毎回剥がして、e2e は必ず seed.base.sql で initialize する。
# scripts/build.sh が生成した古い webapp/sql/seed.sql が残っていると、seed-data を更新
# しても /api/initialize がそれを優先して dev で挙動が変わらない事故になる。
# 重い 1500 件 seed で測りたい場合は手動で seed-gen full → cargo run の経路を取る。
rm -f "$ROOT/webapp/sql/seed.sql"

echo "==> docker compose up -d mysql"
docker compose up -d mysql

echo "==> wait for mysql healthy"
for _ in $(seq 1 60); do
    STATUS="$(docker compose ps --format json mysql | jq -r '.Health // empty')"
    if [[ "${STATUS}" == "healthy" ]]; then
        break
    fi
    sleep 1
done
if [[ "${STATUS:-}" != "healthy" ]]; then
    echo "FAIL: mysql did not become healthy within 60s (status=${STATUS:-unknown})" >&2
    docker compose ps mysql >&2
    exit 1
fi

echo "==> cargo build (webapp, debug)"
( cd "$ROOT/webapp" && cargo build )

echo "==> start webapp in background"
DATABASE_URL="mysql://isucon:isucon@127.0.0.1:3307/nrb2026" \
    "$ROOT/webapp/target/debug/webapp" &
WEBAPP_PID=$!
trap 'kill ${WEBAPP_PID} 2>/dev/null || true; wait ${WEBAPP_PID} 2>/dev/null || true' EXIT

echo "==> wait for webapp readiness (GET /healthz)"
# bench 自身が冒頭で /initialize を叩くので、ここでは生死確認だけ済ませる。
# /initialize は seed 流し込みで時間がかかる上に bench の責務でもあるので、
# liveness probe には使わない。
for _ in $(seq 1 30); do
    if curl -fsS --max-time 1 http://127.0.0.1:8080/healthz >/dev/null 2>&1; then
        READY=1
        break
    fi
    if ! kill -0 ${WEBAPP_PID} 2>/dev/null; then
        echo "FAIL: webapp process died before becoming ready" >&2
        exit 1
    fi
    sleep 1
done
if [[ "${READY:-0}" -ne 1 ]]; then
    echo "FAIL: webapp did not become ready within 30s" >&2
    exit 1
fi

echo "==> run bench"
"$ROOT/scripts/dev-bench.sh"
