# nrb2026 default role
#
# 適用順序:
#   common      → apt 更新、sysctl、SSH 設定
#   users       → isucon ユーザー作成
#   mysql       → MySQL 8.0 install + nrb2026 db + isucon user
#   rustup      → rustup バイナリと toolchain 投入
#   webapp      → webapp ソース配置 + 事前 cargo build + systemd unit (cargo run debug, :80)
#   bench       → bench バイナリ + bench.sh 配置
#   benchwarmer → /opt/benchwarmer/* + systemd unit (install のみ、enable は CFn bootstrap)
#   isuwari     → /opt/isuwari/* + systemd unit (install のみ、enable は CFn bootstrap)
#
# nginx / tls cookbook は nrb2026 では使わない (= webapp が axum で 80 番直配信、TLS なし)。
# 「アプリで static file を配るのは遅い」ことに参加者が気づき、nginx 等を導入する流れ自体が
# 改善経路の 1 本になっている。詳細: docs/authoring/platform.md 付録 A。
#
# benchwarmer / isuwari unit は intentionally disable のまま AMI に焼く: 役割
# (benchmarker 機 vs SUT 機) は CFn UserData の bootstrap.sh が決める。SUT 機では
# isuwari 起動確定後に bench / benchwarmer を bootstrap.sh が削除する。
#
# 残り placeholder cookbook (docker/envoy/nftables/ipaddr) は順次有効化する。
ENV['DEBIAN_FRONTEND'] = 'noninteractive'

include_recipe '../cookbooks/common/default.rb'
include_recipe '../cookbooks/users/isucon.rb'
include_recipe '../cookbooks/mysql/default.rb'
include_recipe '../cookbooks/rustup/default.rb'
include_recipe '../cookbooks/webapp/default.rb'
include_recipe '../cookbooks/bench/default.rb'
include_recipe '../cookbooks/benchwarmer/default.rb'
include_recipe '../cookbooks/isuwari/default.rb'
