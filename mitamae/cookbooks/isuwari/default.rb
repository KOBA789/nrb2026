# isuwari は各 SUT に常駐するエージェント。longseat に keepalive を張って portal の
# Reboot()/Ping() RPC を待ち受ける。/opt/isuwari/{isuwari,bootstrap.sh} を AMI に焼き、
# systemd unit を install するが **enable はしない** (= 役割確定は CFn UserData の
# bootstrap.sh で行う、SUT 機側のみ enable される、benchmarker 機側ではこの unit は
# install されたまま放置される = enable されないので無害)。
#
# isuwari 自身は systemctl reboot を発行するため User=root で動かす
# (= ../../../../isunarabe2/docs/spec/contest-lifecycle.md § 8.7 で root 持ちは許容済、
# "isuwari は team の root 配下にある ... 性善説で受容")。
# 将来 sudoers + 非 root 化は別 PR (= isunarabe2 の binary 実装依存)。
#
# 追試 invariant: SUT を再起動すると systemd が isuwari を auto-start する
# (= bootstrap.sh が enable した状態で reboot 後も維持される、platform.md § 9)。
#
# binary は scripts/build.sh が ../isunarabe2/ workspace を `cargo build --release` して
# files/opt/isuwari/isuwari に staging する。

directory '/opt/isuwari' do
  owner 'root'
  group 'root'
  mode  '0755'
end

remote_file '/opt/isuwari/isuwari' do
  owner 'root'
  group 'root'
  mode  '0755'
end

remote_file '/opt/isuwari/bootstrap.sh' do
  owner 'root'
  group 'root'
  mode  '0755'
end

# systemd unit は install のみ (= 役割確定は bootstrap.sh が行う)。
file '/etc/systemd/system/isuwari.service' do
  owner 'root'
  group 'root'
  mode  '0644'
  content <<~UNIT
    [Unit]
    Description=ISUNARABE isuwari (SUT agent)
    After=network-online.target
    Wants=network-online.target

    [Service]
    Type=simple
    User=root
    EnvironmentFile=/etc/default/isuwari
    ExecStart=/opt/isuwari/isuwari
    Restart=on-failure
    RestartSec=5

    [Install]
    WantedBy=multi-user.target
  UNIT
end
