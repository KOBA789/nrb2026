#!/bin/bash
# SUT 機 (192.168.0.{11,12,13}) の cloud-init runcmd から呼ばれる。
#
# CFn UserData が SETUP_TOKEN / INSTANCE_INDEX / ISUWARI_PUBLIC_IP env を渡してくる。
# ISUWARI_PUBLIC_IP は CFn 側で attach した EIP の値で、isuwari Ping に乗せて portal
# に報告される (= チームが SSH/HTTP 先を portal 上で見つけるため)。
# /api/setup/bootstrap で 10y TeamToken に交換し、systemd EnvironmentFile
# (/etc/default/isuwari) に書いて isuwari.service を enable --now する。
# isuwari binary は INSTANCE_INDEX=0..=2 を起動時に検証する (spec § 8.5、不正値で
# exit non-zero)。
#
# isuwari 起動が成功した場合に限り、AMI に焼かれている bench / benchwarmer 関連を
# 削除する: 競技者の AI agent が SUT 上で偶発的にバイナリを発見してシナリオを解析
# するのを防ぐ。フォレンジック対策は不要 = rm のみ (設計判断)。
#
# 削除順序が重要: enable --now 失敗時に bench を消すと SUT が完全に死ぬので、
# is-active で起動確認した後でのみ削除する。set -e で早期 exit すれば bench は残る。
set -Cue -o pipefail

: "${SETUP_TOKEN:?SETUP_TOKEN required}"
: "${INSTANCE_INDEX:?INSTANCE_INDEX required}"
: "${ISUWARI_PUBLIC_IP:?ISUWARI_PUBLIC_IP required}"

TEAM_TOKEN=$(curl -fsS --retry 5 --retry-connrefused --retry-all-errors \
    --max-time 10 --connect-timeout 5 \
    -H "Authorization: Bearer ${SETUP_TOKEN}" \
    "https://api.isunarabe.org/api/setup/bootstrap")

# atomic 書き込み: 詳細は benchwarmer/bootstrap.sh のコメント参照。
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
cat >> "$tmp" <<EOF
LONGSEAT_URL=https://longseat.isunarabe.org
TEAM_TOKEN=${TEAM_TOKEN}
INSTANCE_INDEX=${INSTANCE_INDEX}
ISUWARI_PUBLIC_IP=${ISUWARI_PUBLIC_IP}
RUST_LOG=info
EOF
install -m 0640 -o root -g isucon "$tmp" /etc/default/isuwari

systemctl daemon-reload
systemctl enable --now isuwari
# enable --now は Type=simple だと ExecStart の即座 exit を捕まえないので明示確認
systemctl is-active --quiet isuwari

# isuwari 起動確定後にのみ bench 関連を削除する。unit ファイル本体も消す
# (mask だと unit 定義が残って解析素材になる)。
rm -rf /opt/bench /opt/benchwarmer
rm -f /etc/systemd/system/benchwarmer.service
systemctl daemon-reload
