# ISUCON 競技規範

ISUCON 競技として守られる規範 (作問者が知っておくべき競技者目線のルールとベンチマーカー慣習) を
集約する。本家 ISUCON 過去問のベンチマーカーと公式講評を一次資料とする。

## 1. 追試 (post-validation) のレギュレーション

競技者が変更後も維持すべき invariant と失格条件。ISUCON 13 / 14 で確立した現代標準を
default として整理する。

### 1.1 ISUCON 13 / 14 の追試手順 (現代標準)

ISUCON 14 manual.md `## 追試` (440-457) と ISUCON 13 cautionary_note.md (470-484) で
ほぼ同型の手順が定義されている。合同演習2026 もこれを default とする想定:

1. ポータル登録の 3 台を再起動。1 台でも再起動できなければ **失格**。
2. envcheck で環境一致を確認。失敗で **失格**。
3. 全 3 台が同一 VPC でない (= 複数スタック混在) なら **失格** (14 のみ明示)。
4. 最終スコアを記録したサーバを target に負荷走行。FAIL もしくは「最終スコアの **75% 以下**」なら
   再実行。3 回目でも基準未達なら **失格**。
5. 全 3 台を再起動 (2 回目)。1 台でも再起動失敗なら **失格**。
6. 負荷走行中に書き込まれたデータが再起動後に取得できなければ **失格**。
7. アプリフロントエンドにアクセスし「構造がリファレンス実装と異なる / 意図しないデータが
   表示されている」なら **失格**。

### 1.2 5 回横断の比較

| 項目 | 9q | 12q | 12f | 13 | 14 |
|---|---|---|---|---|---|
| 75% 再現スコア判定 | × | × | × | ◎ | ◎ |
| envcheck | × | × | × | ◎ | ◎ |
| 永続性 (全チーム) | ◎ (manual.md:289) | ◎ | ◎ | 上位のみ | ◎ |
| フロント表示破損で失格 | ◎ | ◎ | ◎ | ◎ | ◎ |
| 終了時 3 台起動必須 | ◎ | ◎ | (5 台) | ◎ | ◎ |
| 計算機資源は CFn 縛り | × (旧 AMI 配布) | ◎ | ◎ | ◎ | ◎ |
| 改変禁止ファイル明示 | `isucon_admin` | — | — | envcheck.service / isuadmin | envcheck.service / isuadmin |
| 初期化タイムアウト | 20s | — | — | (bench 内 timeout) | (bench 内 timeout) |

### 1.3 ベンチ作問者向けの含意

- **追試 = 「再起動 → 再走行 → 永続性確認 → フロント確認」を bench 内で全部やらない**。
  bench は「単発走行 + 初期データ完全一致の再検査口」を提供し、再起動制御と「75% 以上か」の
  判定は portal/運営に委譲する。これは [research/bench-survey-14.md](research/bench-survey-14.md) の
  `--only-post-validation` モードが明確な実例。
- **「75% 以下で失格」を採点に焼き込まない**。bench 自身は毎回フルスコアを返し、portal が
  本走スコアと再走スコアの比で判定する。bench に閾値を持たせると競技中の誤判定リスクが大きい。
- **永続性は最低限 portal で見る、深いものは作問チーム手動**。13 で「上位チームに対してのみ」
  深い永続性追試を行う方針が採られているのは、bench 内に永続性検査を組まないことの裏返し。

## 2. 変更してよいもの / だめなもの

5 回横断で観測される「公式マニュアル + ベンチ実装」の境界を、合同演習2026 の作問判断材料として
整理する。原文は ISUCON 14 manual.md「変更してはいけない点」(135-145) と
ISUCON 13 cautionary_note.md「実環境において変更してはいけない点」が代表。

### 2.1 変更してよいもの

- **DB スキーマ / インデックス / 初期データ**: `POST /api/initialize` で初期状態に戻せる範囲なら
  自由。bench は initialize 後の状態を pretest で検査するので、index 追加・カラム追加・テーブル
  分割は許容される (機能等価性は要維持)。
- **アプリケーション言語の差し替え**: 参考実装の何れか (Go / Node / Perl / PHP / Python /
  Ruby / Rust 等) に切替可。`/initialize` レスポンスの `language` フィールド送信が必須
  (12q `cmd/bench/main.go:246` ハードコードバグの教訓と裏腹に、portal report の必須項目)。
- **ミドルウェア構成**: nginx / DB / app の配置・台数分担・キャッシュ層追加など。
- **セキュリティグループ**: 必要なポート開放は可。ただし SSH 22 と「外部公開しているアプリ
  ポート」は変更禁止が定番 (本家 ISUCON は HTTPS 443、13 では DNS 53 も追加。nrb2026 は
  HTTP 80)。
- **OS パッケージ・カーネル設定**: 制限なし。

### 2.2 変更してはいけないもの (= 改変で失格)

- **API レスポンスの外部仕様**: JSON フィールド名 / 型 / セマンティクスは固定。
  9q manual.md (172-) は「APIが一度に返す商品数は初期実装と同じを保つ」など量的不変まで指定。
- **アプリ機能の等価性**: 「古いデータを非表示にする」「即座に反映が要求される取得 API を遅延
  させる」等は不可 (9q manual.md:178-)。bench の pretest がこれを焼く。
- **フロントエンド表示の構造**: ISUCON 14 manual.md:456-457「構造がリファレンス実装と異なる、
  意図しないデータが表示されている」で失格。bench 14 は frontend SHA を embed して検査
  ([research/bench-survey-14.md](research/bench-survey-14.md))。
- **CFn テンプレートで作成されたリソースの仕様**: インスタンスタイプ変更、台数追加・削減、
  リージョン変更は失格。「外部資源の利用はモニタリング・テスト・開発に限り、スコア向上効果
  を持つものは禁止」が共通条文。
- **追試系のシステムファイル** (13 / 14):
  - `/etc/systemd/system/envcheck.service`
  - `/etc/systemd/system/multi-user.target.wants/envcheck.service`
  - `/opt/isucon-env-checker/` 配下
  - 13 では `aws-env-isucon-subdomain-address.service` も追加
  - `isuadmin` (旧称 `isucon_admin`) ユーザのアカウント・権限・ログイン情報
- **題材固有の検証鍵**: 12q では `webapp/public.pem` (JWT 検証用公開鍵) のような bench が
  検証に使う鍵ファイルを明示的に固定。

### 2.3 作問者向けチェックリスト

合同演習2026 で作問する際、規範境界を引くため以下を決めておく:

- [ ] アプリ仕様書 (= 競技者向け公開) で「保証する外部仕様」を JSON Schema 級に明文化する
- [ ] CFn テンプレートで作成するリソース型とその不変性条文をマニュアルに書く
- [ ] envcheck で何を見るかを決める
- [ ] フロントエンド表示の検査方法 (SHA hash か目視か) を決める
- [ ] DB の「永続化保証範囲」(load 中追加データ / 初期データ) を決める
- [ ] ベンチ・portal の鍵ファイル (公開鍵・bench トークン) があれば変更禁止に列挙する

## 3. ベンチマーカーの典型挙動

歴代 ISUCON 5 セット (9q / 12q / 12f / 13 / 14) のベンチマーカー実装から、
合同演習2026 の bench を新規設計するときに参考になる典型・慣習・テクニック・落とし穴を
抽出する。深掘りした一次調査は本節末尾に列挙する research/ 配下を参照。

### 3.1 5 セット横断で観察された慣習

5 セット中 3 セット以上で同じ選び方を取っていた設計パターン。合同演習2026 で迷ったときの
default 選択肢として参考になる。

- **phase の責務分離: 「pretest = 整合性検査専念、load = 性能/scoring 専念」** (9q / 12q / 12f / 13 / 14 全回)。
  - pretest 段で意味的な field 値・状態遷移・business invariant を 1 度きり厳しく検査し、load 段は
    status code + JSON 復号性程度の浅い検査と scoring counter 更新に絞る、という二極化。
  - 12q だけは「load 中も小さな Validate worker を常駐させる」変形を取るが、それも「load の主役は
    scoring、整合性検査は別 worker」という責務分離の系列。
- **二段階のエラー分類 (critical / soft) + 件数閾値での fail fast** (5 全回)。
  - critical (`ErrCritical` / `CriticalError` / `internal-error-*` / world.IsCriticalError 等) は 1 件で fail。
  - soft (`ErrApplication` / `NormalError` / `scenario-error-*` / soft warning 等) は閾値方式
    (9q: application 10 件 / 12q: 減点率 100% / 12f: 50 件 / 14: 200 件)。
  - 「予期しないエラー」は critical に倒す保守的判定がしばしば見られる (9q `fails/fails.go:91-94`)。
- **永続性検査 (再起動後にデータが残っているか) は bench 外 (portal 追試 / 運営手動) に委譲** (5 全回)。
  - bench 内の最終チェックは「あったとしても初期データ完全一致や決済の追い込み程度」に限定し、
    再起動を伴う追試は portal 側の job dispatch protocol で「再起動後にもう一度 bench を走らせて
    再現スコア (典型 75% 以上) を見る」形が定型。
  - 14 の `--only-post-validation` モードは「初期データのみ再検査する口を bench 側に提供する」
    例外的な実装で、それでも「load 中に追加されたデータの永続性」までは bench で見ない。
- **scoring は ScoreTag × 倍率の線形加点 + エラー件数 × 固定減点** (12q / 12f / 13 / 14)。
  - portal 連携も protobuf `BenchmarkResult` を `ISUXBENCH_REPORT_FD` に書き戻す形に揃っている
    (12q / 12f / 14、13 は `BenchResult` JSON だが Tip 合計のみで構造は同型)。
- **失敗系 (異常系) pretest を 4〜6 件入れて business invariant を critical 化する** (9q / 12q / 12f / 13)。
  - 「自分の商品は買えない」「不正 CSRF / 期限切れ JWT / 残高不足カードで決済失敗」「`pipe`
    ユーザ登録拒否」「ban ユーザのログイン拒否」など、正常系では到達しない「拒否されるべき経路」
    を 1 シーケンスにまとめる手法。
- **load の並列度を成功カウンタ・エラー率・競技者操作で動的に引き上げる** (5 全回)。
  - 9q: `/initialize` で競技者が宣言する campaign 値 / 12q: AdminBilling 周回完了で worker spawn /
    12f: login 成功カウンタの階段 / 13: DNS 攻撃の攻撃強度を解決成功率で 1.5 倍ずつ増加 /
    14: 売上に応じて Owner が Chair を増やす。
- **初期データ + 期待値を bench に同梱** (9q / 12q / 12f / 14)。
  - JSON dump (`benchmarker.json` / `validateUserInitialize.json` / `dump/*.json` / scenario/data 配下)
    と `//go:embed` の組み合わせで、bench が「答え」を webapp 実装と独立に握る形。
- **外部依存サービスの mock を bench プロセスに同居させ、bench 内部から状態を直接 inspect する** (9q / 14。
  12q / 12f / 13 はそもそも対応する外部サービスを持たない題材)。
  - bench 内 HTTP server として起動し、`ForceSet` / `Verify` / `GetReports` 等で内部状態を直接読み書き。
  - 9q payment / shipment、14 payment が代表例。

### 3.2 設計バリエーションのトレードオフ

同じ目的を 5 セットで別手法で解いた事例。合同演習2026 の方針判断材料として整理する。

#### (A) pretest と load の検査の厚みの分担

| 回 | pretest | load 中の整合性検査 | 最終チェック |
|---|---|---|---|
| 9q | verify 9 シナリオ並列 | Check 4 シナリオ常駐 | bench 内 final-check (payment mock vs `/reports.json` 突合) |
| 12q | Prepare で 6 種検査並列 | Validate worker 4 種が load と並走 | bench 内 final-check 無し |
| 12f | Prepare に整合性検査 12 段を全寄せ | status code + JSON decode のみ | `Scenario.Validation` は `return nil` の空実装 |
| 13 | Pretest 13 関数を逐次実行 | scoring counter 更新のみ | `FinalcheckScenario` は事実上 stub |
| 14 | 初期データ完全一致 + frontend SHA | 世界モデルで通知・座標・売上を常時突合 | `paymentServer.Close()` + 5 秒猶予で決済追い込み |

合同演習2026 で選ぶときの判断材料:

- **「pretest に全寄せ + load は scoring 専念」(12f / 13 系)**: bench 実装が単純で fail 判定が
  シンプル。load 中の隠れた regression (たまたま pretest を通り抜けたバグ) を取りこぼす risk
  はあり、それを double-submit 検査などで部分的に補う。
- **「load 中も Validate worker / 世界モデルで継続検査」(12q / 14 系)**: regression 検出は強いが、
  bench の状態管理コストが大きく、race の許容セットを明示的に書く必要が出てくる
  (14 では `world/user.go:394-411` で ride 状態遷移の race をハードコード)。
- **「bench 内 final-check で payment mock などと突合」(9q 系)**: scoring が「外部サービスに記録された
  業務金額」のような確定値で表せる場合に強い。`/reports.json` のような bench-only endpoint を
  webapp に実装させる必要がある。

#### (B) load 並列度の動的引き上げ手法

| 回 | 手法 | 制御変数 |
|---|---|---|
| 9q | 競技者の自己宣言式 | `/initialize` レスポンスの `campaign` 値 (0〜4) |
| 12q | 周回完了 → 新 worker spawn の連鎖 | AdminBilling 1 周 → NewTenant worker、OrganizerJob 完了 → PlayerScenarioWorker |
| 12f | 成功カウンタの階段 | login 成功数 / user 登録成功数で `1, 3, 6, 9, 12, 15` 階段、エラー増分 > 5 で停止 |
| 13 | DNS 解決成功率 | 失敗率 < 1% かつ avg 解決数 > 50 で攻撃並列度を 1.5 倍 (上限 15) |
| 14 | entity 数の自然増殖 | 売上に応じて Owner が Chair を増やす、評価高いライドで User を招待増 |

合同演習2026 では「単一 client の処理速度を測定するなら固定並列度で十分、捌ける限り重みを上げる
形にしたいなら成功カウンタ階段 (12f 方式) が素直」が baseline。世界モデル方式 (14) を採るなら
entity 増殖が自然に並列度を上げる。

#### (C) 期待値の握り方

| 回 | 期待値の表現 |
|---|---|
| 9q | 静的ファイル md5 はディレクトリ参照式 (起動時スキャン)、商品データは asset の md5 を持つ |
| 12q | `benchmarker.json` / `benchmarker_tenant.json` を bench に同梱、billing 計算式を bench 内で再現 |
| 12f | `dump/validateUserInitialize.json` で「次にログインしたら login bonus seq がいくつ進むか」まで事前計算 |
| 13 | `StatsSched` (インメモリ世界モデル) で webapp と並行に統計を再計算 |
| 14 | scenario/data embed JSON (4 セッションぶん) + 世界モデルで全 entity 状態を bench 側に持つ |

期待値を「ハードコードすると bench メンテナンスが重くなる」「JSON 同梱だとデータ生成器との
同期が必要」「世界モデルだと bench 実装コストが大きい」のいずれかのコストを払う構造。
合同演習2026 が小規模な作問物なら同梱 JSON 方式が無難で、複雑な状態遷移を題材にするなら
13 の `StatsSched` 系が中間解、配車・取引のような能動 entity 中心ドメインなら 14 の世界モデル。

#### (D) 永続性検査と bench 外責務の境界

5 セット全回で永続性検査は portal 追試に委譲する。本選択は「bench 1 走行は再起動を伴わない
1 サイクル測定に絞る」という ISUCON 慣習で、合同演習2026 でも踏襲が default。bench 内に
再起動制御を持たせる選択肢は存在するが、本サーベイの 5 セットでは採用例がない (= benchwarmer /
portal 側の job dispatch プロトコルで担う)。

### 3.3 転用可能な手法カタログ

各 survey §4 に挙がった idiom を 1 リストに集約。合同演習2026 で使えそうなものを優先順で並べる。

| # | idiom | 出典 | 合同演習2026 での想定用途 |
|---|---|---|---|
| 1 | 外部サービス mock を bench プロセス内同居 + `ForceSet` / `Verify` で内部状態 inspect | 9q `bench/server/payment.go,shipment.go` / 14 `bench/payment/` | 決済・配送・通知などの外部 API を題材にする場合の標準形 |
| 2 | session pool でログイン済み session を再利用し、ログイン回数を負荷の主役にしない | 9q `bench/scenario/pool.go` | 認証コストを「主役」にしたくないドメイン全般 |
| 3 | 「sell と buy の間に他エンドポイントを挟む」シナリオ縛り | 9q `bench/scenario/load.go:29-35` | 単一 endpoint だけ最適化する偏ったチートを防ぎたいとき |
| 4 | 静的ファイル md5 の「ディレクトリ参照式」検査 (起動時スキャン) | 9q `bench/scenario/verify.go:322-363` | bench に md5 をハードコードしないメンテナンス容易性 |
| 5 | 同時購入競合のバッファ 1 channel 検査 (「真の勝者は 1 人」) | 9q `bench/scenario/campaign.go:230-232` | 在庫 1 / 抽選 / 排他制御を題材にする場合 |
| 6 | TLS SNI / Host header を target-host フラグで固定 | 9q `bench/session/session.go` | nginx vhost + TLS 終端のシンプル構成 |
| 7 | bench 内蔵 DNS リゾルバ + allowlist 検査 (`miekg/dns` 直叩き) | 13 `internal/resolver/dns.go` | 名前解決を題材にする / 複数台構成で IP allowlist が要る場合 |
| 8 | DNS 水責め攻撃エンジン (`bytebufferpool` + `sync.Pool` + UDP 接続使い回し) | 13 `internal/attacker/dns.go` | 「世界の他の actor が常時負荷を生む」演出が要るとき |
| 9 | `x-isu-date` ヘッダで bench から webapp の time を支配 | 12f `action.go:388-399`, `webapp/go/main.go:147-151` | 時刻依存処理 (バッチ集計 / TTL / cron / ログインボーナス) を題材にする場合 |
| 10 | `x-master-version` 422 + worker の `Rewind` で master 入替を fail なくこなす | 12f `scenario_login.go:35-38, 112-114` | スキーマ進化 / マスター入替を競技中に挟む設計 |
| 11 | 同 token で 2 回叩く double-submit 検査 (整合性検査内に組み込む) | 12f `scenario_validation.go:580-604, 828-850` | ワンタイムトークン / 冪等キー / 1 回限りクーポン |
| 12 | `cmp.Diff` で複雑な集計レスポンスを完全一致比較 (差分文字列を error message に) | 12q `scenario_tenant_billing_validate.go:259` | billing / 統計 / ランキングの「1 ビット違いを検出したい」場合 |
| 13 | マルチテナント ID 範囲別シナリオ排他 (4 区画に切る) | 12q `bench/constants.go:24-31` | 並列 worker が同じ初期データを破壊しないように物理 sharding |
| 14 | 「Slow response でユーザー脱落」モデル (減点ではなく加点機会の喪失) | 12q `scenario_player.go:131-136` | 「重い API は減点ではなく機会損失で罰したい」とき |
| 15 | 走行中も Validate worker を常駐させて整合性検査を継続 | 12q `scenario.go:240-265` | 「pretest だけでは regression を取りこぼす」リスクを許容できないとき |
| 16 | Reproduce flag による「当日バグ込み挙動」の保存 | 12q `scenario_player.go:203` | 競技後に bench を直しつつ当日採点も再現したいとき |
| 17 | 世界モデル方式 (entity が自律的に Tick で行動) | 14 `world/` 配下全体 | 配車 / 取引 / 在庫など状態遷移中心の能動 entity 集団がいるドメイン |
| 18 | payment mock の `Idempotency-Key` 対応 + 確率的 5xx (直近処理数に応じた失敗率) | 14 `payment/handler.go:36-46, 71-127` | 冪等性とリトライを実装させたい場合 |
| 19 | nearby 検査の suspicious 機構 (3 秒後の延期チェックで race を許容しつつ最終判定) | 14 `world/world.go:326-459` | bench とサーバ間の race を許容しつつ最終的に整合性を担保したい場合 |
| 20 | `--only-post-validation` モードで再起動追試の初期データ検査だけ切り出す | 14 `bench/cmd/run.go:92-111` | portal の追試 workflow で「初期化処理の正当性」だけ短時間で確認したい場合 |
| 21 | `--pretest-only` モードで開発時の feedback ループを短縮 | 13 `bench.go:236-239` | 競技者・運営の手元動作確認の standard option |

### 3.4 知っておきたい実装上の落とし穴 (事実列挙)

各 survey §6 に挙がった「code 内の事実」(FIXME / dead code / 計算ミス / 残置コメント) を
カテゴリで集約。合同演習2026 で同じ穴に落ちないための checklist。

- **論理演算子の typo / 数学的に常に false な条件式**: 14 `world/payment.go:28` の
  `if a <= 0 && a > 1_000_000` (`||` typo で該当経路は dead branch)、9q `verify.go:661, 886` の
  「buyer かつ seller が同一 user」分岐など。範囲チェックは `if a < min || a > max` の定型を厳守。
- **`// FIXME` / コメントアウトのまま稼働する分岐**: 13 `core_finalcheck.go:25` の
  「ライブコメント存在チェック」stub、13 `cmd/bench/benchmarker.go:356` の
  `// とめておく bencherror.RunViolationChecker(ctx)`、14 `world/owner.go:243-246` の
  active 状態整合検査の race 回避無効化、12q `scenario.go:286-287` のデバッグ用フィルタ残置。
  「コメントアウトしたら issue を起こす」運用と PR レビュー時の grep を defaults に。
- **dead code / dead constant**: 12q `constants.go:4-5` の `ConstMaxError` / `ConstMaxCriticalError`
  が宣言だけで未参照、14 `prevalidation.go:226` の `validateSuccessFlow` (どこからも呼ばれない)、
  13 `streamer.go:22, 110` の `BasicLongStreamer*` stub。`golangci-lint` の `unused` を CI で有効化。
- **slice / map の初期化と書き込みの取り違え**: 12q 3 箇所で `make([]string, N)` の後に
  `append` して長さ `2N` (前半 N 件は空) になる例。`make([]T, 0, N)` + `append` か
  `make([]T, N)` + index 代入のどちらかに統一。同一 key の `if !ok` 二重書き
  (12q `scenario_validation.go:1495-1501`) も同種の混乱の現れ。
- **集計 / 状態のリセット忘れ・初期化重複**: 13 `internal/benchscore/profit.go:7-11` で
  `profit` グローバル変数が `InitCounter` でリセットされない (bench を 2 phase 連続実行で
  キャリーオーバ)、12q `scenario.go:151, 168` で `GetInitialData` が同一処理で 2 度呼ばれて
  1 回目が即上書き。`InitCounter` 系は「全集計の reset」に集約し、Prepare 段は 1 関数に閉じる。
- **Set 系コンテナで Add と Remove のキー食い違い**: 12f `scenario_login.go:26-33` で
  `Add(int64(trial))` (= インデックス値) と `defer Remove(user.ID)` (= ユーザ ID)。
  生成と解除は 1 関数にカプセル化して逸脱を防ぐ。
- **`rand.Intn(len - 1)` で len=1 のとき panic**: 12f `scenario_validation.go:805` の
  `rand.Intn(len(gachaData) - 1)` パターン。`if len < 2 { return }` の早期 return か、
  `rand.Intn(len)` への変更で安全側に。
- **goroutine 内 write と外側 check の race**: 12q `scenario.go:430-441` の `playerWorkerKick` が
  goroutine 内で lock + map write する一方、check 側も同 lock で読むため、同一 key で worker が
  複数立ち上がる可能性。書き込み完了を channel ack で同期する。
- **`/initialize` レスポンスの language を portal report に流し忘れ**: 12q
  `cmd/bench/main.go:246` の `Language: "galaxy"` ハードコード (TODO コメント付き)。
  bench テンプレートでは `/initialize` 取得 → portal report 流し込みを 1 セットにする。
- **log 出力関数の使い間違い**: 14 `postvalidation.go:19` で `slog.String(key, value)` の戻り値を
  捨てて何も出力されない例。`slog.String` は `Attr` を返すコンストラクタなので
  `slog.Error("...", slog.String(...))` の形で渡す。
- **マジックナンバーの分散とコメント残置**: 9q `load.go:482, 571, 717` の
  「`MEMO 50件よりはみないだろう`」、12q `MaxErrors = 30` / 12f `MaxErrors = 50` など、
  bench 内の数値しきい値は constants 1 ファイルに集約する。
- **mock サーバの「意図された stub」には明示コメントを**: 9q `payment.go:249-251, server.go:88-100`
  の「未検証で信じる」「コピペしないこと」(`// DO NOT COPY`) のような注記スタイルを参考に、
  合同演習2026 で類似 mock を書く場合も意図を明示しておく。

### 3.5 参照

詳細な一次調査は research/ 以下を参照。`path:line` 形式の引用や具体的なシナリオ展開もそちらに
集約してある。

- [research/bench-survey-9q.md](research/bench-survey-9q.md)
- [research/bench-survey-12q.md](research/bench-survey-12q.md)
- [research/bench-survey-12f.md](research/bench-survey-12f.md)
- [research/bench-survey-13.md](research/bench-survey-13.md)
- [research/bench-survey-14.md](research/bench-survey-14.md)

## 4. スコア式の設計慣習

加点 / 減点 / FAIL 条件の典型と、合同演習2026 のスコア式を組むための判断軸。
詳細な実装抽出は [research/bench-survey-*.md](research/) を、回ごとの内訳は
本節下の表を参照。

### 4.1 5 回のスコア式まとめ

| 回 | 加点モデル | 減点モデル | 倍率 |
|---|---|---|---|
| 9q | 取引完了売上の直加 (`取引価格合計 - 減点`) | error 1 件 -500 / timeout 200 件超で 100 件毎 -5000 | なし |
| 12q | ScoreTag × 倍率 (更新系 10 / 参照系 1) | NormalError 1% / CriticalError 10% (率) | あり |
| 12f | ScoreTag × 倍率 (login 3 / gacha 2 / home 1 等) | scenario-error 1 件 -15 | あり |
| 13 | Tip (投げ銭) 単純加算 | なし (log 表示のみ) | なし |
| 14 | `(fare + distance × 10) / 100` を Owner ごと加算 | なし (Deduction 0 固定) | あり (distance) |

**進化軌跡**: 後発ほど「減点の廃止」と「business metric を直接加点」が強まる。12q の
「エラー率 (%) で fail 判定」は一度きりで、以降は「critical 1 件で fail / soft error は件数閾値」に
回帰している。

### 4.2 FAIL 判定の共通パターン

- **critical エラー 1 件で即 FAIL** (5 回全回共通)。critical の種類はドメイン固有
  (業務不変条件 / 状態遷移 / 決済整合 / DNS allowlist 違反など)。
- **soft / application エラーは件数閾値** (9q: 10 件 / 12q: 減点率 100% / 12f: 50 件 /
  13: 200 件 / 14: 200 件)。
- **Prepare (pretest) 段で 1 件でも失敗で FAIL** (isucandar 仕様)。
- **減点で 0 点以下に達したら FAIL** (9q manual.md:272 が明示。12f は max(0, sum) で丸める
  方式に変更)。
- **永続性検査の失敗は bench ではなく portal/運営判定** (5 回全回)。

### 4.3 最終スコアの取り方

- **競技中最終ジョブのスコアを最終スコアとする** (5 回全回共通)。複数走行の平均や最高は採らない。
- **最後のジョブが FAIL なら受賞対象外** (14 manual.md:371)。
- **追試で 75% 未満 = 失格 (= ランク外)** であって、再走スコアそのものを最終スコアにはしない。

### 4.4 合同演習2026 でスコア式を組むときの判断軸

[research/bench-survey-*.md](research/) の知見を統合した、迷ったときの選び方:

- **加点モデルは「business metric の直加」を default に**。13 (Tip 合計) / 14 (売上)
  / 9q (取引高) の流れ。Tag × 倍率方式 (12q / 12f) は「ドメインに自然な金額単位がない場合」の
  fallback。
- **減点を入れるかは「failure を可視化する文化」と「競技バランス」のトレードオフ**。
  14 の「減点なし」は bench が world model で error を critical 化する設計と表裏一体。
  作問物が単純なら減点を持つほうが competitor feedback として教育的。
- **critical の選定はドメイン依存だが、「業務不変条件 (= 二重決済・amount 改ざん・在庫超過) は
  必ず critical」が default**。state 遷移 (有限オートマトンの逸脱) も critical 候補。
- **倍率を入れるなら自然に降りてくる量で**。14 の `distance × 10` (距離長いほど価値ある
  ride) のように、ドメインに「重み付け」が自然に存在するときのみ。人工倍率は競技者から見て
  ノイズになる。

### 4.5 学生賞 / 言語賞 / 企業賞 (補足)

ISUCON 13 / 14 ではスコアそのもの以外の特別賞が並走している。作問者が直接設計するのは
スコア順位の本選であって特別賞のレギュレーションではないが、選考にスコア以外の評価軸が
入ることは知っておく:

- **学生賞**: 学生のみで構成されるチームの最高スコア (13 / 14 で実施)。
- **言語賞**: 各言語実装での最高スコア。`/initialize` レスポンスの `language` フィールドが
  選考根拠。「言語の標準実装で何点取れたか」の参考値になるため、言語間の素朴な公平性は
  作問段階で気にしておく価値がある。
- **企業賞**: スポンサー企業がドメイン特性で別軸選考 (例: 13 は「DNS 攻撃耐性」が企業賞の
  評価軸として明言、cautionary_note.md:464-466)。bench に「スコアに加算しないが計測する
  サブ指標」を仕込むことが企業賞の温床になる。

合同演習2026 での扱いは [project.md](../project.md) の運営方針次第だが、bench 側で
「メイン指標と独立な計測値を出力する余地」は残しておくと後付けで賞を作りやすい。

## 5. レギュレーション原文 / 講評記事の参照

合同演習2026 の作問判断時に直接当たれる一次資料を厳選した。ローカル (本リポジトリ)
にあるものはパスで、それ以外は URL で示す。

### 5.1 マニュアル原文 (ローカル)

`../isucon/` 配下に過去問のリポジトリが clone されている前提:

- **9q**: `../isucon/isucon9-qualify/docs/manual.md` (失格条件 / 減点 / 追試 = 257-294 行)
- **13**: `../isucon/isucon13/docs/cautionary_note.md` (追試 = 470-484 行)
- **14**: `../isucon/isucon14/docs/manual.md` (追試 = 436-457 行) +
  `../isucon/isucon14/docs/ISURIDE.md` (アプリ仕様)
- **12q / 12f**: マニュアル原文は portal 配信のみで repo に同梱されていない (講評記事を参照)

### 5.2 公式講評 (isucon.net)

- [ISUCON14 問題の解説と講評](https://isucon.net/archives/58869617.html) — Pocket Sign tohutohu。
  スコア式 (`0.1 × 配車距離 + 移動距離 + 完了 ride × 5`) と 30ms tick world model の一次資料。
- [ISUCON13 問題の解説と講評](https://isucon.net/archives/58001272.html) — kazeburo (さくら)。
  スコアを Tip に絞った設計判断。
- [ISUCON12 本選問題の解説と講評](https://isucon.net/archives/56959385.html) — goodoo
  (CyberAgent)。マスタキャッシュ + Snowflake ID + 想定スコアと当日実績の比較。
- [ISUCON12 予選問題の解説と講評](https://isucon.net/archives/56850281.html) — fujiwara
  (カヤック)。マルチテナント SaaS 題材を予選に出した経緯と想定解。
- [ISUCON9 予選問題の解説と講評](https://isucon.net/archives/53789931.html) — catatsuy ら
  (メルペイ)。スコア指標を売上ベースにした思想の一次資料。

### 5.3 作問担当者ブログ

- [ISUCON14作問後記:ベンチマーカーが正しすぎて盛り上がった話](https://tech.pocketsign.co.jp/entry/2025/02/26/180622)
  — Pocket Sign。「bench を正しく作る」ことで「ハック余地が小さく、本質で戦う」問題に
  なったという設計判断。本リポジトリの bench 設計指針 (不整合検出の正当性最優先) と直結。
- [ISUCON13のベンチマーカーのDNS水責め攻撃について](https://kazeburo.hatenablog.com/entry/2023/12/02/235258)
  — kazeburo。「スコアに直接効かない負荷」を仕込む設計と参加者向け対処パターン 5 種。
- [本選のみ開催のISUCON13、問題はどう変わる？](https://flatt.tech/magazine/entry/20231107_isucon13_question)
  — Flatt Magazine による作問 3 名インタビュー。「初級者にも入口を残しつつ差がつくボトル
  ネックを置く」哲学。
- [ISUCON9予選の出題と外部サービス・ベンチマーカーについて](https://catatsuy.medium.com/isucon9-qualify-969c3abdf011)
  — catatsuy。外部 payment/shipment を bench 内蔵してチームごとに分離した設計判断。
- [isucandarとISUCON9予選ベンチマーカーについて](https://zenn.dev/catatsuy/articles/500a437427fedf281c23)
  — catatsuy。フレームワーク化を避け単純に書いた判断と、その経験から切り出された isucandar
  設計。**bench を新規作成するなら必読**。
- [ISUCON9予選でフロントエンド周りの実装を担当した話](https://sota1235.hatenablog.com/entry/2019/10/07/110500)
  — sota1235。「機能が理解しやすい」を最優先に SPA を設計した方針。本リポジトリの
  「フロントは競技者がアプリを理解するためのもの」(CLAUDE.md) と一致。

### 5.4 アーカイブ (まとめページ)

- [ISUCON14 まとめ](https://isucon.net/archives/58837992.html) — 全チームスコア + 受賞情報
- [ISUCON13 まとめ](https://isucon.net/archives/57801192.html)
- [ISUCON12 予選まとめ](https://isucon.net/archives/56836729.html) /
  [本選結果](https://isucon.net/archives/56923294.html)
- [ISUCON9 予選まとめ](https://isucon.net/archives/53789734.html)
