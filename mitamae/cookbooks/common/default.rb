execute "apt-get update" do
  command "apt-get -y update"
end

# 最小限。各 cookbook が必要に応じて自前で package を追加する。
%w[
  build-essential
  pkg-config
  libssl-dev
  curl
  rsync
  ca-certificates
  jq
].each do |pkg|
  package pkg
end

# namespaced sysctl (container でも EC2 でも同じ挙動)
file "/etc/sysctl.d/99-isu.conf" do
  owner "root"
  group "root"
  mode "0644"
  content "net.ipv4.ip_local_port_range = 10000 65535\n"
end

execute "sysctl --system"
