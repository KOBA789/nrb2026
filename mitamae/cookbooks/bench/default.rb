directory '/opt/bench' do
  owner 'root'
  group 'root'
  mode '0755'
end

remote_file '/opt/bench/bench' do
  owner 'root'
  group 'root'
  mode '0755'
end

remote_file '/opt/bench/bench.sh' do
  owner 'root'
  group 'root'
  mode '0755'
end

# === bench fixture (椅子写真 20000 枚) ===
# 個別ファイルを remote_directory で展開すると payload/Packer/mitamae 適用が
# 重くなるため、zip 1 ファイルで配って AMI 上で sentinel 付きで展開する。
#
# **mitamae の remote_file は file 全体を Ruby メモリに乗せるため、1.58 GB の
# zip を渡すと OOM で殺される (= 4 GB RAM の VM で実測)**。tar 展開後の payload
# パス (`/tmp/payload/.../initial.zip`) から `execute` で直接 unzip する形を取る。
# scripts/build.sh と Packer ami.pkr.hcl の両方が `/tmp/payload` に展開する。
package 'unzip'

directory '/opt/bench/fixtures' do
  owner 'root'
  group 'root'
  mode '0755'
end

execute 'unpack bench fixtures' do
  # tmp dir に展開してから rename: 途中失敗時は .ready が touch されないので
  # 冪等に retry できる。zip は payload からそのまま読んで AMI には残さない。
  command <<~CMD
    set -e
    src=/tmp/payload/mitamae/cookbooks/bench/files/opt/bench/fixtures/initial.zip
    test -f "$src" || { echo "ERROR: bench fixture zip not found at $src" >&2; exit 1; }
    tmp="$(mktemp -d /opt/bench/fixtures/unpack.XXXXXX)"
    trap 'rm -rf "$tmp"' EXIT
    unzip -qq "$src" -d "$tmp"
    rm -rf /opt/bench/fixtures/images
    mv "$tmp/v3_initial_data" /opt/bench/fixtures/images
    touch /opt/bench/fixtures/.ready
  CMD
  not_if 'test -f /opt/bench/fixtures/.ready'
end
