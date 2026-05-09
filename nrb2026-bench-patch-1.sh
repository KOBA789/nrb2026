#!/usr/bin/env bash
# nrb2026-bench-patch-1.sh
#
# 配布済み bench AMI (= benchmarker 機) に対して、benchwarmer.service の
# LimitNOFILE を 1048576 に引き上げるパッチ。bench インスタンス上で root 権限で
# 1 回実行すれば反映される。冪等。
#
# 背景: 初期の AMI は systemd デフォルトの open files 上限 (soft 1024) のままで、
# 高並列時に bench プロセスが "too many open files" を踏むケースがあるため。

set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  exec sudo -E "$0" "$@"
fi

if ! systemctl cat benchwarmer.service >/dev/null 2>&1; then
  echo "ERROR: benchwarmer.service が見つかりません。bench インスタンス上で実行してください。" >&2
  exit 1
fi

DROPIN_DIR=/etc/systemd/system/benchwarmer.service.d
DROPIN=$DROPIN_DIR/10-nofile.conf

install -d -m 0755 "$DROPIN_DIR"
tmp=$(mktemp "$DROPIN_DIR/.10-nofile.conf.XXXXXX")
trap 'rm -f "$tmp"' EXIT
cat >"$tmp" <<'EOF'
# nrb2026-bench-patch-1: open files 上限の引き上げ
[Service]
LimitNOFILE=1048576
EOF
chmod 0644 "$tmp"
mv -f "$tmp" "$DROPIN"
trap - EXIT

systemctl daemon-reload

if systemctl is-active --quiet benchwarmer.service; then
  systemctl restart benchwarmer.service
  echo "benchwarmer.service を再起動しました。"
else
  echo "benchwarmer.service は停止中。次回起動時に反映されます。"
fi

echo
echo "==== systemctl show benchwarmer.service (LimitNOFILE) ===="
systemctl show benchwarmer.service \
  --property=LimitNOFILE --property=LimitNOFILESoft

while read -r pid comm; do
  [[ -z $pid ]] && continue
  echo
  echo "==== /proc/$pid/limits ($comm) ===="
  grep 'Max open files' "/proc/$pid/limits" || true
done < <(pgrep -af '^/opt/bench/bench( |$)' || true)

echo
echo "OK: LimitNOFILE=1048576 を反映しました。"
