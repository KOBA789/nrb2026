# benchwarmer は portal からの job dispatch を longseat 経由で受けて bench.sh を exec する
# エージェント。/opt/benchwarmer/{benchwarmer,problem.json,bootstrap.sh} を AMI に焼き、
# systemd unit を install するが **enable はしない** (= 役割確定は CFn UserData の
# bootstrap.sh で行う、benchmarker 機側のみ enable される、SUT 機側ではこの cookbook の
# 産物ごと bootstrap.sh が削除する)。
#
# binary は scripts/build.sh が ../isunarabe2/ workspace を `cargo build --release` して
# files/opt/benchwarmer/benchwarmer に staging する。

directory '/opt/benchwarmer' do
  owner 'root'
  group 'root'
  mode  '0755'
end

remote_file '/opt/benchwarmer/benchwarmer' do
  owner 'root'
  group 'root'
  mode  '0755'
end

remote_file '/opt/benchwarmer/bootstrap.sh' do
  owner 'root'
  group 'root'
  mode  '0755'
end

remote_file '/opt/benchwarmer/problem.json' do
  owner 'root'
  group 'root'
  mode  '0644'
end

# systemd unit は install のみ (= 役割確定は bootstrap.sh が行う)。
file '/etc/systemd/system/benchwarmer.service' do
  owner 'root'
  group 'root'
  mode  '0644'
  content <<~UNIT
    [Unit]
    Description=ISUNARABE benchwarmer
    After=network-online.target
    Wants=network-online.target

    [Service]
    Type=simple
    User=isucon
    Group=isucon
    EnvironmentFile=/etc/default/benchwarmer
    ExecStart=/opt/benchwarmer/benchwarmer
    Restart=on-failure
    RestartSec=5
    LimitNOFILE=1048576

    [Install]
    WantedBy=multi-user.target
  UNIT
end
