#!/usr/bin/env bash
# Local VM test for nrb2026 AMI provisioning.
#
# 既定: fresh cycle — stop → user-data 更新 → reset-disk → start → SSH 待ち →
# cloud-init 待ち → payload 転送 → mitamae apply → bench 実行を 1 回。
#
# --reuse: stop/reset/start を skip し、起動中の VM に対して payload 転送 →
# apply → bench だけを行う。cookbook の高速試行錯誤用。
#
# 仕様: docs/authoring/build-pipeline.md § 8-9
# kkapi: docs/kkapi.md
set -euo pipefail

KKAPI="${KKAPI:-http://10.200.6.254:7878}"
VM_NAME="${VM_NAME:-isu-dev}"
VM_IP="${VM_IP:-10.200.6.100}"
SSH_USER="${SSH_USER:-ubuntu}"
BASE_IMAGE="${BASE_IMAGE:-noble-server-cloudimg-amd64-disk-kvm-20260321.img}"
DISK_SIZE="${DISK_SIZE:-30G}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

REUSE=0
case "${1:-}" in
    --reuse) REUSE=1 ;;
    -h|--help)
        grep -E '^# ' "$0" | sed 's/^# \?//' | head -20
        exit 0
        ;;
    "") ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
esac

if ! curl -fsS "${KKAPI}/vms/${VM_NAME}" >/dev/null; then
    echo "ERROR: cannot reach kkapi at ${KKAPI} (or VM '${VM_NAME}' not found)" >&2
    exit 1
fi

"$ROOT/scripts/build.sh"

mkdir -p "$ROOT/build"
KNOWN_HOSTS="$ROOT/build/known_hosts"
SSH_OPTS=(
    -o BatchMode=yes
    -o StrictHostKeyChecking=accept-new
    -o "UserKnownHostsFile=${KNOWN_HOSTS}"
    -o ConnectTimeout=5
)

if [[ $REUSE -eq 0 ]]; then
    cat > "$ROOT/build/user-data.yaml" <<'YAML'
#cloud-config
manage_etc_hosts: false
package_update: false
package_upgrade: false
runcmd:
  - systemctl disable --now apt-daily.timer apt-daily-upgrade.timer
YAML

    echo "==> stop VM ${VM_NAME} (idempotent)"
    curl -fsS -X POST "${KKAPI}/vms/${VM_NAME}/stop" >/dev/null

    echo "==> PUT user-data"
    curl -fsS -X PUT \
        -H 'Content-Type: text/plain' \
        --data-binary @"$ROOT/build/user-data.yaml" \
        "${KKAPI}/vms/${VM_NAME}/user-data" >/dev/null

    echo "==> reset-disk (base=${BASE_IMAGE}, size=${DISK_SIZE})"
    curl -fsS -X POST \
        -H 'Content-Type: application/json' \
        --data-binary "{\"base\":\"${BASE_IMAGE}\",\"size\":\"${DISK_SIZE}\"}" \
        "${KKAPI}/vms/${VM_NAME}/reset-disk" >/dev/null

    rm -f "${KNOWN_HOSTS}"

    echo "==> start VM"
    curl -fsS -X POST "${KKAPI}/vms/${VM_NAME}/start" >/dev/null

    echo "==> wait for SSH (${VM_IP})"
    for _ in $(seq 1 180); do
        if ssh "${SSH_OPTS[@]}" "${SSH_USER}@${VM_IP}" true 2>/dev/null; then
            break
        fi
        sleep 0.5
    done

    echo "==> wait for cloud-init"
    ssh "${SSH_OPTS[@]}" "${SSH_USER}@${VM_IP}" 'cloud-init status --wait' >/dev/null
fi

echo "==> rsync payload"
rsync -a -e "ssh ${SSH_OPTS[*]}" "$ROOT/build/payload.tar.gz" "${SSH_USER}@${VM_IP}:/tmp/payload.tar.gz"

echo "==> mitamae apply"
ssh "${SSH_OPTS[@]}" "${SSH_USER}@${VM_IP}" 'bash -se' <<'REMOTE'
set -e -o pipefail
cd /tmp
rm -rf payload
tar xzf payload.tar.gz
sudo /tmp/payload/mitamae/mitamae local /tmp/payload/mitamae/roles/default.rb
# 配備済 (= /opt/bench/fixtures/.ready などが既に作られている) ので
# /tmp の staging artifact (~3 GB) は破棄してディスクに戻す。
sudo rm -rf /tmp/payload /tmp/payload.tar.gz
REMOTE

echo "==> wait for webapp readiness (GET /healthz)"
ssh "${SSH_OPTS[@]}" "${SSH_USER}@${VM_IP}" '
    for _ in $(seq 1 60); do
        if curl -fsS --max-time 3 http://127.0.0.1/healthz >/dev/null 2>&1; then exit 0; fi
        sleep 2
    done
    echo "webapp did not become ready within timeout" >&2
    sudo systemctl --no-pager status nrb2026-webapp.service >&2 || true
    exit 1
'

# SPA 配信 + /api 404 fallback の smoke。bench は /api/* しか叩かないため、SPA 経路の
# 配備事故 (= dist 未 staging / STATIC_DIR 未設定 / SPA fallback が /api を吸う) は
# bench だけでは検出できない。build pipeline 段階での backstop。
echo "==> verify SPA root + asset body + /api 404 (= SPA に吸われない)"
ssh "${SSH_OPTS[@]}" "${SSH_USER}@${VM_IP}" '
    set -e
    body="$(curl -fsS --max-time 3 http://127.0.0.1/)"
    echo "$body" | grep -q "<div id=\"root\"></div>" || {
        echo "FAIL: / did not return SPA index.html (no React root mount point)" >&2
        echo "--- body ---" >&2
        printf "%s\n" "$body" >&2
        exit 1
    }
    asset_path="$(printf "%s\n" "$body" | grep -o "/assets/[^\"]*" | head -1)"
    [ -n "$asset_path" ] || {
        echo "FAIL: / did not reference Vite-built /assets/ (dist staging mismatch?)" >&2
        exit 1
    }
    curl -fsS --max-time 3 "http://127.0.0.1${asset_path}" >/dev/null || {
        echo "FAIL: referenced asset did not resolve: ${asset_path}" >&2
        exit 1
    }
    code="$(curl -s -o /dev/null -w "%{http_code}" --max-time 3 http://127.0.0.1/api/__no_such_endpoint__)"
    [ "$code" = "404" ] || {
        echo "FAIL: /api/__no_such_endpoint__ returned $code (expected 404 — SPA fallback consuming /api/*)" >&2
        exit 1
    }
'

echo "==> run bench (REPORT_FD=3 → /tmp/report.json)"
ssh "${SSH_OPTS[@]}" "${SSH_USER}@${VM_IP}" '
    set -e
    rm -f /tmp/report.json
    REPORT_FD=3 \
    BENCHWARMER_target_index=0 \
    BENCHWARMER_target_ip=127.0.0.1 \
    BENCHWARMER_all_ips=127.0.0.1 \
    WEBHOOK_URL=http://127.0.0.1:9999/webhook \
        /opt/bench/bench.sh 3>/tmp/report.json
    cat /tmp/report.json
'

SCORE="$(ssh "${SSH_OPTS[@]}" "${SSH_USER}@${VM_IP}" 'jq -r .score < /tmp/report.json')"
echo "score=${SCORE}"
if [[ -z "${SCORE}" ]] || [[ "${SCORE}" -lt 1 ]]; then
    echo "FAIL: bench reported score < 1 (or unparsable)" >&2
    exit 1
fi
echo "OK: bench reported score=${SCORE}"
