# rustup 本体だけ install する。toolchain 自体は webapp cookbook が
# rust-toolchain.toml を見て on-demand に install する。

execute "install rustup for isucon user" do
  command <<~SH.strip
    curl --proto '=https' --tlsv1.3 -sSf https://sh.rustup.rs \
      | sudo -u isucon -H sh -s -- -y --no-modify-path --default-toolchain none
  SH
  not_if "test -x /home/isucon/.cargo/bin/rustup"
end
