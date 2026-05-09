# webapp ソースを /home/isucon/webapp/ に配置し、cargo build で target/ を温め、
# systemd unit (cargo run = debug) で起動する。
#
# ベンチ序盤の改善パス「debug → release」がそのまま参加者に見える形になっている。

# remote_directory は rsync --delete 相当ではないため、hashed asset chunk が
# 旧 deploy から残らないよう SPA 配信先だけ先に空にする。
execute 'clear stale SPA assets' do
  command 'rm -rf /home/isucon/webapp/public'
end

remote_directory '/home/isucon/webapp' do
  source 'files/home/isucon/webapp'
  owner 'isucon'
  group 'isucon'
end

# rust-toolchain.toml を起点に toolchain を install。これが初回 cargo 呼び出し。
execute 'install rust toolchain via rustup auto-detection' do
  command 'sudo -u isucon -H bash -lc "cd /home/isucon/webapp && /home/isucon/.cargo/bin/cargo --version"'
end

# debug build を事前に走らせて target/ を温める。AMI 起動時の cargo run が即座に走るように。
execute 'pre-build webapp (debug, warms target/)' do
  command 'sudo -u isucon -H bash -lc "cd /home/isucon/webapp && /home/isucon/.cargo/bin/cargo build"'
end

# nrb2026 では nginx を置かず webapp が直接 80 番で SPA + API を配信する設計。
# 80 (privileged port) を非 root ユーザ isucon で bind するため AmbientCapabilities=
# CAP_NET_BIND_SERVICE を unit で渡す。systemd は uid 切替後も ambient を保つので、
# cargo → webapp の exec を経由しても継承される。
# SPA (frontend/dist) は STATIC_DIR=/home/isucon/webapp/public を参照して axum の
# ServeDir + index.html fallback で配信する (BrowserRouter 対応)。dist は
# scripts/build.sh が pnpm build して payload に staging する。
# 詳細: docs/authoring/platform.md 付録 A。
file '/etc/systemd/system/nrb2026-webapp.service' do
  owner 'root'
  group 'root'
  mode '0644'
  content <<~UNIT
    [Unit]
    Description=nrb2026 webapp (debug build via cargo run, listens on :80)
    After=network-online.target
    Wants=network-online.target

    [Service]
    Type=simple
    User=isucon
    Group=isucon
    WorkingDirectory=/home/isucon/webapp
    Environment=PATH=/home/isucon/.cargo/bin:/usr/local/bin:/usr/bin:/bin
    Environment=DATABASE_URL=mysql://isucon:isucon@127.0.0.1:3306/nrb2026
    Environment=PORT=80
    Environment=STATIC_DIR=/home/isucon/webapp/public
    AmbientCapabilities=CAP_NET_BIND_SERVICE
    CapabilityBoundingSet=CAP_NET_BIND_SERVICE
    NoNewPrivileges=true
    ExecStart=/home/isucon/.cargo/bin/cargo run --bin webapp
    Restart=on-failure
    RestartSec=2

    [Install]
    WantedBy=multi-user.target
  UNIT
end

execute 'systemctl daemon-reload'

service 'nrb2026-webapp' do
  action [:enable, :start]
end
