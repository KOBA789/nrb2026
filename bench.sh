#!/usr/bin/env bash
# AMI 上では mitamae の bench cookbook が /opt/bench/bench.sh にコピーする。
# benchwarmer から exec され、REPORT_FD と benchwarmer 由来の env を受け取る。
# 詳細は docs/authoring/platform.md § 5 を参照。
set -euo pipefail
export BENCH_FIXTURES_DIR="${BENCH_FIXTURES_DIR:-/opt/bench/fixtures/images}"
# 本番 webapp は :80 で listen する (nginx 廃止 + axum 直配信、docs/authoring/platform.md 付録 A)。
# bench source の default は dev-loop と整合する 8080 のため、production は明示する。
export WEBAPP_PORT="${WEBAPP_PORT:-80}"
# webapp が叩く webhook receiver は bench プロセス自身 (この node の :9999)。
# bench source の default は target_ip:9999 (= SUT 側) を指してしまうため、production
# topology (Benchmarker = 192.168.0.100、docs/authoring/platform.md § topology) に揃える。
# local test (test-local.sh) では呼出側で WEBHOOK_URL を上書きする。
export WEBHOOK_URL="${WEBHOOK_URL:-http://192.168.0.100:9999/webhook}"

export BENCH_AUDITED_NEW_ACTORS_MAX=1000
export BENCH_AUDITED_ACTIVE_ACTORS_MAX=1000
export BENCH_NOTIFICATION_ACTORS_MAX=1000
export BENCH_INITIAL_ACTORS_PER_KIND=3
export BENCH_RAMP_STEP_JOINS=3
exec /opt/bench/bench 2>&1
