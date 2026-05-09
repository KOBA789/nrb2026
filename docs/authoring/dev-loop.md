# 作問者向け: ローカル開発ループ

webapp / bench の反復を host 上の `cargo run` で完結させるためのループ。VM
(`scripts/test-local.sh`) は provisioning / cookbook / systemd 周りを触る時のみ使う。

## 構成

```
bench (cargo run --release) ──HTTP:8080──▶ webapp (cargo run) ──sqlx:3306──▶ MySQL 8.0 (compose)
```

- MySQL は `compose.yaml` で `mysql:8.0` を **`127.0.0.1:3307`** に立てる
  (host 側に既存 mysqld が居る環境を想定して 3306 から退避してある。production AMI 側は
  3306 のまま)。`mysql_native_password` 認証 + `nrb2026` DB + `isucon/isucon` ユーザを env
  で作成。
- webapp は `DATABASE_URL=mysql://isucon:isucon@127.0.0.1:3307/nrb2026` を export してから
  `cargo run`。bench は env を渡さずそのまま動く。

## 前提

- Docker (compose v2)、Rust toolchain (`rust-toolchain.toml` で pin)、`jq`、`curl`。
- host の TCP 3307 と 8080 が空いていること
  (host に mysqld が居ても干渉しないように 3307 にずらしてある)。
- host に `mysql` client (Ubuntu なら `mysql-client` / `mariadb-client`)。webapp の
  `POST /api/initialize` が schema/seed を流すために `mysql` を `Command` で起動する。
  AMI 側は `mysql-server` package に同梱されるので追加 install 不要。

## 反復ループ (ロジックを触る時)

```bash
# 1. MySQL を起動 (1 度きり、以降は up したまま)
docker compose up -d mysql

# 2. webapp を前景起動 (logs を見ながら反復)
DATABASE_URL=mysql://isucon:isucon@127.0.0.1:3307/nrb2026 cargo run --manifest-path webapp/Cargo.toml

# 3. 別端末で bench を 1 発
scripts/dev-bench.sh
```

`scripts/dev-bench.sh` は release build → `BENCHWARMER_target_ip=127.0.0.1` で `bench` を実行
→ `build/dev-report.json` に `{"score": N}` を出し、score < 1 なら exit 1。

## ワンショット e2e (smoke test)

```bash
scripts/e2e.sh
```

compose の up を idempotent に確認 → webapp を bg 起動 → readiness を poll → `dev-bench.sh`
→ trap で webapp を kill。CI 風に「ビルドが壊れていないか」だけ素早く見たい時に。

## 経路の使い分け

| やること | 経路 | 所要 |
|---|---|---|
| webapp ロジックの反復、bench で挙動確認 | ローカル (`cargo run` + `dev-bench.sh`) | コンパイル増分のみ |
| webapp + bench の smoke check | `scripts/e2e.sh` | 初回 webapp build + 3 秒 bench |
| mitamae cookbook / systemd unit / cloud-init | `scripts/test-local.sh` | --reuse ~10 秒、fresh ~75 秒 |
| AMI 最終検証 | `scripts/build-ami.sh` | Packer 数分 |

## seed の再生成

`webapp/sql/seed.base.sql` (dev fallback、commit 対象) と
`webapp/sql/seed.sql` (配布版、gitignored) は seed-gen が `seed-data` 定数から
生成する。`seed-data/src/lib.rs` の UUID 定数を変えたら fallback を再生成する:

```bash
cargo run --release -p seed-gen -- base --out webapp/sql/seed.base.sql
```

`/api/initialize` は `seed.sql` があればそれを優先するが、`scripts/e2e.sh` は冒頭で
`webapp/sql/seed.sql` を必ず削除するので、e2e 経路では古い配布版 seed が混ざる事故は
起きない。配布版 (1500 件 + 実画像) は `scripts/build.sh` が AMI ビルド時に
`seed-gen full` で再生成する。dev で重い seed.sql に当てて測りたい場合は手動で
生成し、`scripts/e2e.sh` ではなく `cargo run` + `scripts/dev-bench.sh` 経路を使う:

```bash
cargo run --release -p seed-gen -- full \
    --count 1500 --seed 0xC0FFEE \
    --fixtures bench/fixtures/images \
    --out webapp/sql/seed.sql
```

## 既知の制約

- compose の MySQL volume は名前付き (`mysql_data`)。データを完全に消したい時は
  `docker compose down -v`。
- `scripts/e2e.sh` は webapp を debug build で立ち上げる。release build と挙動を比較したい
  時は手で `cd webapp && cargo run --release` してから `dev-bench.sh` を叩く。
- VM (`test-local.sh`) との差は MySQL がコンテナか実体かのみ。auth plugin は揃えてあるので
  sqlx の挙動差は出ないはず。

## 参照

- 作問規範とベンチマーカー慣習 → [norms.md](norms.md)
