# bench fixtures

bench / seed_gen が使う椅子写真 fixture ディレクトリ。

リポジトリには **含めない**。`scripts/fetch-bench-fixtures.sh` で
ISUCON9 qualify v2 release から `initial.zip` を DL → `images/` に展開する。
`scripts/build.sh` から自動的に呼ばれる。

## 出典

- **ISUCON9 qualify** ([github.com/isucon/isucon9-qualify](https://github.com/isucon/isucon9-qualify))
  - リポジトリ License: MIT (Copyright (c) 2019 ISUCON)
  - 使用 release: `v2` (`initial.zip`)
- **椅子画像提供**: 941-san
  ([@941, 2019 年提供](https://twitter.com/941/status/1157193422127505412))

## 規模

- 20,000 枚の JPEG (500x500、`<MD5>.jpg` 命名)
- サイズ: 約 50 KiB 〜 153 KiB / 枚 (平均 ~78 KiB、全件 ≤ 200 KiB)
- 合計: 約 1.6 GB

## ライセンス上の注意

ISUNARABE 合同演習 2026 は本家 ISUCON とは無関係の非公式演習。9q
リポジトリは MIT License。MIT の表記要件を満たすためには、配布物に
本 README を同梱して出典を明示すれば十分。
