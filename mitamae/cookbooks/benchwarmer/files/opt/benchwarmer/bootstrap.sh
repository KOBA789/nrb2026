#!/bin/bash
# benchmarker 機 (192.168.0.100) の cloud-init runcmd から呼ばれる。
#
# CFn UserData が SETUP_TOKEN env を渡してくる。/api/setup/bootstrap で 10y TeamToken
# に交換し、systemd EnvironmentFile (/etc/default/benchwarmer) に書いて
# benchwarmer.service を enable --now する。
#
# 失敗時は curl 失敗または systemctl exit non-zero で set -e により早期 exit。
# benchwarmer.service が起動しなくても、AMI 上の bench / problem.json は触らない
# (benchmarker 機側は意図的に解析されても許容範囲、SUT 側 bootstrap (= isuwari) と
# 対称的に振る舞う)。
set -Cue -o pipefail

: "${SETUP_TOKEN:?SETUP_TOKEN required}"

TEAM_TOKEN=$(curl -fsS --retry 5 --retry-connrefused --retry-all-errors \
    --max-time 10 --connect-timeout 5 \
    -H "Authorization: Bearer ${SETUP_TOKEN}" \
    "https://api.isunarabe.org/api/setup/bootstrap")

# atomic 書き込み: mktemp で 0600 root:root の暫定ファイルを作り、内容を入れた上で
# install -m 0640 -o root -g isucon で目的地に rename する。途中段階で世界読み出し
# 可能になる窓を作らない。set -C 下では `>` が tempfile に直接効かないので `>>` を使う
# (mktemp は空ファイルを作る)。
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
# PORTAL_URL は h2c (= http://) で接続する。isunarabe2 の tonic = 0.14 は TLS feature
# 無しで build されており (benchwarmer/Cargo.toml)、`Endpoint::from_shared` で https://
# を渡すと runtime で接続失敗する。process-compose / harness も `http://` 規約。
# 公開 endpoint で h2c を直接終端する想定 (ALB の TLS termination → backend h2c か、
# 別 hostname/port での expose かは isunarabe2 側のデプロイ判断に依存)。
cat >> "$tmp" <<EOF
LONGSEAT_URL=https://longseat.isunarabe.org
PORTAL_URL=https://api.isunarabe.org
TEAM_TOKEN=${TEAM_TOKEN}
PROBLEM_DESCRIPTOR=/opt/benchwarmer/problem.json
BENCH_PROGRAM=/opt/bench/bench.sh
RUST_LOG=info
EOF
install -m 0640 -o root -g isucon "$tmp" /etc/default/benchwarmer

systemctl daemon-reload
systemctl enable --now benchwarmer
