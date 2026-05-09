# ISUCON 12 予選 (12q / ISUPORTS) ベンチマーカー survey

## 0. メタ情報

- 競技日: 2022-07-23
- 言語 (一次): Go (`webapp/go/isuports.go`、Echo + sqlx + MySQL/SQLite)
- bench code root: `../isucon/isucon12-qualify/bench/` (CLI: `bench/cmd/bench/main.go`)
- webapp Go root: `../isucon/isucon12-qualify/webapp/go/` (主に `isuports.go` 1620 行)
- manual source:
  - 当日マニュアル: <https://gist.github.com/mackee/4320c18919c8f6f1867849378a17e651>
  - アプリケーションマニュアル (ISUPORTS): <https://gist.github.com/mackee/460eeb8040389ed5bdeaf2c48327707c>
- 競技ドメイン: マルチテナント SaaS (大会運営プラットフォーム)。テナント DB は SQLite ファイル (`tenantDBPath` = `../tenant_db/{id}.db`、`isuports.go:74`) で隔離、共通の MySQL に admin DB がある。

## 1. 全体構造

### Phase 構造

isucandar 1.x の `Prepare → Load` の 2 段構造 (`benchmark.Start(ctx)` を `cmd/bench/main.go:104` で呼ぶ)。bench 内に明示的な FinalCheck (post-load validation) phase は存在しない。

- **Prepare** (`scenario.go:86` `func (sc *Scenario) Prepare`)
  - 60 秒タイムアウト (`scenario.go:88`)
  - JWT 鍵 (`isuports.pem`) と初期データ JSON (`benchmarker.json` / `benchmarker_tenant.json`) のロード (`scenario.go:146-171`)
  - `POST /initialize` 実行と `lang` フィールド検証 (`scenario.go:176-189`)
  - `ValidationScenario` を 1 回回す (`scenario.go:192`)
  - 失敗時は `failure.NewError(ErrFailedPrepare, ...)` で portal に `fail` を返す
  - `--prepare-only` フラグでここで打ち切り可能 (`cmd/bench/main.go:178-184`)
- **Load** (`scenario.go:204` `func (sc *Scenario) Load`)
  - 起動時に 6 種の永続 worker を `WorkerCh` にキック (`scenario.go:213-265`):
    AdminBilling / PopularTenant(軽い) / NewTenant / TenantBillingValidate / AdminBillingValidate / PlayerValidate
  - 以後、`AdminBillingScenario` と `OrganizerJob` の進行に応じて新規 PopularTenant(重い)・NewTenant・PlayerScenarioWorker を逐次 spawn (`scenario_admin_billing.go:139-153`、`job_organizer.go:223-228`)
  - `select` ループで worker / Error / Critical / 5 秒ログを処理 (`scenario.go:273-308`)
  - 減点率 100% 到達でその場で打ち切り (`scenario.go:310-314`)

### Scoring 概観

加点は ScoreTag を `step.AddScore` した数 × `ResultScoreMap` の倍率 (`tags.go:41-56`、`scenario_util.go:40-54`)。

- 加点 (重み): 更新系 (`POST .../tenants/add` 等 6 種) は 10 点、参照系 (`GET .../ranking` 等 7 種) は 1 点。
- 減点: NormalError 1% / CriticalError 10% (`cmd/bench/main.go:204`)。減点率 100% で fail。
- FAIL の典型形:
  - Prepare で `ValidationScenario` 失敗 → `ErrFailedPrepare` → `existFailLog=true` (`cmd/bench/main.go:129、214`)
  - Load 中に減点率 100% → `step.AddError(ErrFailedLoad)` (`scenario.go:312`)
  - 最終 `score < 0` または `existFailLog` → `isPassed=false`、`reason="fail"` (`cmd/bench/main.go:209-219`)

### CLI / 環境変数

`cmd/bench/main.go:51-62`:

| flag | default | 用途 |
|---|---|---|
| `-target-url` | `https://t.isucon.dev` | base URL (subdomain 構築用) |
| `-target-addr` | "" | host:port を直接指定 (TLS SNI 解決用) |
| `-request-timeout` | 30s | 通常リクエスト |
| `-initialize-request-timeout` | 30s | `/initialize` 専用 |
| `-duration` | 1m | Load 走行時間 |
| `-exit-error-on-fail` | false | fail 時に exit 1 |
| `-prepare-only` | false | Prepare 後に終了 |
| `-skip-prepare` | false | Prepare をスキップ |
| `-data-dir` | `data` | (実質未使用) |
| `-debug` | false | デバッグログ |
| `-strict-prepare` | **true** | true なら Validate 中 1 件失敗で即 abort、false なら一部のみ continue |
| `-reproduce` | false | 予選当日の (バグ込みの) 挙動を再現する PlayerScenario 切替 (`scenario_player.go:33-44`) |

環境変数: `ISUXBENCH_TARGET` (supervisor 起動時の対象 host、`cmd/bench/main.go:69`)、`ISUXBENCH_REPORT_FD` (portal 報告 FD、`cmd/bench/main.go:233`)、`ISUCON_JWT_KEY_FILE` (デフォルト `./isuports.pem`、`scenario.go:146`)。

### 入出力 (portal protocol)

`isucon12-portal/bench-tool.go/benchrun` の `Reporter` で `BenchmarkResult` (passed / score / score_breakdown / execution.reason / survey_response.language) を `ISUXBENCH_REPORT_FD` の FD に protobuf で書く (`cmd/bench/main.go:233-249、278-286`)。

## 2. ドメインと擁護する不変条件

### 2.1 ベンチが「絶対に守らせる」business invariant

`AddCriticalCount` を呼んでいる (CriticalError として 10% 減点される、ひいては fail に直結する) 観察点を集めると、bench は次の不変条件を critical 級として擁護している:

- **テナント追加が成功すること**: `AdminBillingScenario` 内の新規テナント追加失敗で `AddCriticalCount` (`scenario_admin_billing.go:133`)。`NewTenantScenario` / `TenantBillingValidate` でも同様 (`scenario_new_tenant.go:69`、`scenario_tenant_billing_validate.go:111-137`)。
- **大会作成 / 終了 / スコア入稿 / 失格 などの Organizer 更新系 API が成功すること**: 各シナリオで `// OrganizerAPI 更新系はCritical Error` のコメント付きで `AddCriticalCount` (`scenario_tenant_billing_validate.go:137,181,239`、`scenario_player_validate.go:110,142,330,365,414`、`job_organizer.go:97,201,253`)。
- **失格者は disqualify 反映後 (= 3 秒猶予後) に player 系 3 API すべてで 403 を返すこと**: `PlayerValidateScenario` で 403 を期待し失敗時 `AddCriticalCount` (`scenario_player_validate.go:477,490,503`)。
- **テナント追加直後の参加者一括追加が成功すること**: `scenario_tenant_billing_validate.go:111`。

「整合性検査」(後述 §3.1) で検出する以下も Prepare 中に critical 化される:

- ランキング順位とスコアの並びが入稿仕様 (CSV 後勝ち、score 降順、同点は CSV 先勝ち) と一致 (`scenario_validation.go:842-852, 944-957, 1041-1047`)
- billing が `len(score)*100 + visitor*10` の計算式で正しく合算 (`scenario_validation.go:582-591, 689-696`)
- ランキング上限が 100 件 (`scenario_validation.go:840, 945`)
- `cmp.Diff` ベースのテナント請求レポート完全一致 (`scenario_tenant_billing_validate.go:259`)

### 2.2 責務分担の境界線

| 責務 | 担い手 | 根拠 |
|---|---|---|
| API 整合性検査 (Prepare) | bench | `scenario.go:192` |
| 走行中の整合性検査 (4 worker 常駐) | bench | `scenario.go:240-265` |
| ベンチ性能スコア算出 | bench | `cmd/bench/main.go:204-209` |
| 言語自動取得 | bench (`/initialize` のレスポンスから読むだけ) | `scenario.go:182`、TODO は `cmd/bench/main.go:246` (`SurveyResponse.Language` がハードコード `"galaxy"`) |
| **再起動による永続性検査** | bench 外 (portal/運営の追試) | bench 内に再起動・再走行ロジックは無い。manual の「追試」節で portal 側の追試 (再起動 + 再走行で 85% 以上のスコア) が規定されている |
| **frontend (browser) UI 確認** | 運営手動 (上位チームのみ追試) | manual「追試」節。bench は HTML / JS / CSS の単一ファイルが返ってくるかだけを Prepare 内 `staticFileCheck` で確認 (`scenario_validation.go:133-186`) し、画面動作は見ない |
| `/api/me` 等 bench から叩かれない API | 競技者の最適化対象 (削除可能) | bench は `/api/me` を呼ばない (action.go に対応関数なし、`webapp/go/isuports.go:180` には route のみ) |
| bench 終了後のテナント DB / MySQL 永続性 | portal 追試 | bench は走行終了で `step.Cancel()` し終了 (`scenario.go:321`) |
| 不正利用 / 失格判定の運用 | 運営手動 | manual「禁止事項」節 |
| isucon-env-checker / blackauth | 運営側 (変更禁止) | manual「変更してはいけない点」節 |

### 2.2.1 bench から叩かれない API (= 競技者の最適化対象 / 運営手動 / portal 委譲)

- `GET /api/me` — bench は呼ばない (action.go に対応関数なし、`webapp/go/isuports.go:180` には route のみ)。事実として bench は叩かないため、競技者が削除しても採点には影響しない。
- 走行中に発火しない `staticFileCheck` の対象 (`/index.html`, `/js/*.js`, `/css/*.css`) — Prepare で 1 度だけ叩く。走行中は呼ばれないため、SPA としての動作確認は bench 責務外 (運営手動の上位チーム追試に委ねられている)。

## 3. 検査の構造

### 3.1 整合性検査 (Prepare)

発火点は `Prepare` → `ValidationScenario` (`scenario_validation.go:31`)。最初に新規テナント 1 個を追加 (直列) してから `errgroup` で 6 関数を並列実行:

1. `allAPISuccessCheck` (`:190`) — admin / organizer / player の 3 ロールで一連の API を叩き、テナント作成 → プレイヤー 100 名追加 → 一覧取得 → 失格設定 → スコア入稿 (CSV) → ranking 取得 (`getRankingFunc` を 4 並列でスコア入稿と同時に走らせ lock の取り忘れを検出、`:391-397`) → finish (3 秒待機後) → ranking / billing 整合 (`:518-599`)。billing の計算式 `players * 100 + visitor * 10` を厳密に検証 (`:582-591`)。
2. `rankingCheck` (`:711`) — 101 人投入 → ranking が 100 件上限になり 1 〜 100 位の order が CSV 順と一致することを 3 パターン (順向き / 逆向き / 同点 + 重複 PlayerID) で確認 (`:838-855, 940-958, 1029-1054`)。
3. `badRequestCheck` (`:1061`) — 重複テナント名 / 大文字テナント名 (400)、存在しない player (404)、不正 CSV (`カラム順 / 余計な列`、400) など 8 通り (`:1128-1219, 1227-1322`)。
4. `invalidJWTCheck` (`:1329`) — exp 切れ JWT / 不正 RSA 鍵 / 不正アルゴリズム (RS512) で 401 を期待 (`:1334-1406`)、存在しないテナント / プレイヤーで 401 (`:1435-1460`)。
5. `billingAPISuccessCheck` (`:1465`) — `before` カーソル付きで `GET /api/admin/tenants/billing` を叩き、初期データ tenant の billing と一致を確認 (`:1502-1504`)。
6. `staticFileCheck` (`:133`) — `isucon` テナントから `/index.html` と `js/`, `css/` の最初の 1 ファイルずつを取得し、ファイルの bytes が `../public/...` と完全一致 (`:170-185`)。

`StrictPrepare=true` (default) では各サブ検証の `if !v.IsEmpty() && sc.Option.StrictPrepare` 分岐により 1 件失敗で即 return。`-strict-prepare=false` 時は一部の検証 (主に「お行儀の良い」境界値と読み取り系の不一致) が継続するが、結果として `step.Result().Errors` が 1 件でもあれば `:125-128` で abort される。`run_ci.sh` は `-strict-prepare=false` を渡しており、CI 用途で緩めている。

### 3.2 負荷走行 (Load)

`scenario.go:204-326` で 6 シナリオを `WorkerCh` 経由で起動し、`select` ループで監視。シナリオは 8 タグ (`tags.go:60-69`) で計測される。

| シナリオ (worker) | 並列度 | ループ | 役割 |
|---|---|---|---|
| `AdminBillingScenarioWorker` (`scenario_admin_billing.go:24`) | 1 | InfinityLoop | `/admin/billing` をページネーションで全部辿り、終わったら新規テナント追加 → そのテナントへ NewTenant worker を spawn |
| `PopularTenantScenarioWorker` (`scenario_popular_tenant.go:21`) | 1 (起動時) → 重テナント用が後から +1 | InfinityLoop, MaxParallelism=20 | 初期テナント (id=2〜29 のいずれか or `isucon`) で `OrganizerJob` を回す |
| `NewTenantScenarioWorker` (`scenario_new_tenant.go:22`) | 1 → AdminBilling 周回完了で逐次 spawn | InfinityLoop, UnlimitedParallelism | 新規テナントで playersAdd → `OrganizerJob` を反復 |
| `TenantBillingValidateWorker` (`scenario_tenant_billing_validate.go:25`) | 1 | LoopCount=3, UnlimitedParallelism | 新規テナント + 100 player を作って大会内 BillingReport を `cmp.Diff` で完全一致検証 |
| `AdminBillingValidateWorker` (`scenario_admin_billing_validate.go:23`) | 1 | InfinityLoop, UnlimitedParallelism | 既存テナント (id=40〜69 範囲) で大会を 1 個閉じてから `/admin/billing` の合計金額が `+ScoredPlayerNum*100` 増えていることを確認 |
| `PlayerValidateScenarioWorker` (`scenario_player_validate.go:25`) | 1 | InfinityLoop, MaxParallelism=1 | 既存テナント (id=70〜99) で「大会作成 → スコア入稿 → 失格 → 3 秒待機 → 失格者の 3 API すべて 403」までを 1 ループで検証 |
| `PlayerScenarioWorker` (`scenario_player.go:25`) | 1 (per player) | LoopCount=1 (実体は内部 `for {}` で長時間動く) | `OrganizerJob` の進行に応じて参加者として spawn される。ranking → player 詳細を見続ける |

**並列度の動的増加**: テナントが追加されるごとに `NewTenantScenarioWorker` が新規に立ち、各 `OrganizerJob` が完了するごとに `PlayerScenarioWorker` が複数 spawn される設計 (`job_organizer.go:212-232`)。

**バックオフ**: シナリオエラー時 `SleepWithCtx(ctx, SleepOnError)` (= 1s, `scenario.go:25`)。各 ranking poll の `getRankingFunc` 等は 100ms〜2s の jitter sleep。

**Retry**: `RequestWithRetry` (`action.go:221-266`) は 429 のときだけ `Retry-After` ヘッダ (delay-seconds か http-date) を読んで再試行する。`PostAdminTenantsAddAction` と `PostOrganizerCompetitionsAddAction` のみ採用 (= 直列性が必要な書き込みでの 429 を吸収)。

**Fail fast 条件**:
- 減点率 100% (`(normalErrorCount * 1) + (criticalErrorCount * 10) >= 100`) 到達で `step.AddError(ErrFailedLoad)` し抜ける (`scenario.go:310-314`)。
- 最大表示エラー数 30 (`MaxErrors`、`scenario.go:23`、コンテスタント側のログのみ。判定には影響しない)。

**Slow response の検出 (PlayerScenario 固有)**: ranking 取得が 1.2 秒以上を 3 回連続で `PlayerDelCountAdd(1)` し worker を脱落させる (`scenario_player.go:131-136`、`:275-281`)。「離脱したプレイヤーが N 人」のログ表示 (`scenario.go:361-372`) で contestant に視覚的フィードバック。

### 3.3 最終チェック / 永続性検査の所在

**bench 内に明示的な final-check / post-load validation phase は存在しない**。`isucandar.NewBenchmark` の `Load` が `option.Duration` で打ち切られた後は `step.Cancel()` → `wg.Wait()` → main で `result.Errors.Wait()` するだけで、追加の API 呼び出しは行わない (`scenario.go:321-324`、`cmd/bench/main.go:106-107`)。

「走行中の Validate worker」(`TenantBillingValidate` / `AdminBillingValidate` / `PlayerValidate`) が無限ループで動き続けることで、「最後の 1 ループ」が事実上の final check の役目を兼ねている。これは isucandar の `Prepare → Load` 構造を踏襲した結果の設計。

**永続性検査** (再起動後にデータが残っているか) は §2.2 のとおり bench 外 (portal の追試) に委譲。当日マニュアル「追試」節で「サーバー再起動後の再走行で 85% 以上 + 上位チームのみデータ永続性追試」が portal/運営側責務として規定されている。

## 4. 採用された手法 (再利用候補)

### マルチテナント ID 範囲別シナリオ排他

`bench/constants.go:24-31` で初期データ範囲を 4 つに切り、各 worker が触ってよい tenant を ID 範囲で排他的に決めている (id=1 重テナント、2-29 PopularTenant 専用 [破壊変更 NG]、30-69 AdminBillingValidate [大会追加 OK]、70-99 PlayerValidate [破壊変更 OK])。並列で走る Validate worker 同士の干渉を「初期データの ID 範囲」だけで sharding している。同種設計を合同演習2026 で取りたい場合、初期データ生成器側で範囲ごとに「破壊 OK / NG」のラベルを `BenchmarkerTenantSource` に持たせる方式が再利用可能。

### 3 ロール × subdomain × JWT cookie

`Account` 構造体 (`action.go:29-40`) が `Role` (admin/organizer/player) と `TenantName` から `{admin|tenantName}.t.isucon.dev` を組み立て (`:46-57`)、JWT を `isuports_session` cookie に注入 (`:103-108`)。これを `GetAccountAndAgent` (`scenario_util.go:123`) で再利用する。bench 内で JWT を自前で発行 (RSA private key を Prepare 時にロード) しており、外部の auth サーバー (`blackauth`) は契約参照のみで bench 検査には使わない。合同演習2026 が JWT/cookie ベース認証を採るなら、本テンプレートをそのまま流用可能。

### isucandar 標準 worker パターン

`worker.NewWorker(fn, worker.WithInfinityLoop(), worker.WithMaxParallelism(N))` を全シナリオで採用し、`SetParallelism(p)` で外部から並列度調整。worker のラッパー型 (`adminBillingScenarioWorker` 等) を 1 つずつ書く慣行。これは 12q 固有でも 12f / 13 / 14 でも見られる定型のはず (要確認)。

### 動的 worker 増殖 (load 中の負荷成長)

`AdminBillingScenario` が完走するごとに新規テナントを追加し → そのテナントへ `NewTenantScenarioWorker` をキック (`scenario_admin_billing.go:139-153`)。`OrganizerJob` 完了で複数 `PlayerScenarioWorker` を追加 (`job_organizer.go:212-232`)。これにより「序盤は軽く、後半は重く」という負荷曲線を競技者の処理能力に応じて自動的に作る。合同演習2026 で時間経過とともに負荷を上げたいときに参考になる。

### Reproduce flag による「当日バグ込み挙動」の保存

`-reproduce` で `PlayerScenario` の代わりに `PlayerScenarioReproduce` (`scenario_player.go:203`) を使う。後者は競技時に「ranking が空ならスリープ無しで連打」していたバグ挙動を残しており、コメントで「この挙動のため PlayerAPI を叩かず ranking で稼ぐ攻略があった」と説明 (`:309-312`)。修正後の bench を競技後に公開しつつ、当日と同じ採点を再現する手段を残すパターン。合同演習2026 で「競技後に bench を直したいが、当時のスコア指標も残したい」場合に応用できる。

### Slow response でユーザー脱落モデル

ranking 1.2s × 3 で worker を 1 体脱落させる (`scenario_player.go:131-136`)。脱落したぶんは「N 人離脱しました」というログだけで、減点ではなく「シナリオ回転数 = 加点機会の喪失」として効く。「重い API は減点ではなく機会損失で罰する」モデル。

### 定常的な「Validate worker」の load 同居

通常の負荷 worker と並行して 4 種の `*Validate` worker が走り続ける (`scenario.go:240-265`)。これは「pretest = 精度、load = 性能」の責務分離 (慣習) に対する 12q なりの折衷で、走行中も常に整合性を検査する。違反は ErrorCh / CriticalErrorCh で減点に変換され、最終的に減点率 100% で fail fast する。この「load 中もずっと小さな pretest を回す」設計は、isucandar 系 bench でしばしば見られる (要確認)。

### `cmp.Diff` で BillingReport を完全一致比較

`scenario_tenant_billing_validate.go:259-261` で `go-cmp` を使い、テナント内 billing report を構造体比較。差分発生時に diff 文字列をそのまま `error` メッセージに含めるためデバッグしやすい。合同演習2026 で「複雑な集計レスポンスの 1 ビット違いを検出したい」ときの定型として使える。

## 5. 設計上の選択点 (横断タグ)

- `[慣習らしい: pretest/prepare = 整合性検査専念、load = 性能/scoring 専念の責務分離 (bench 内 final-check は無い)]` (`scenario.go:86, 204`、`cmd/bench/main.go:104-107`)
- `[慣習らしい: critical / soft (warning) の二段階エラー分類 + soft 件数閾値で critical 化または fail fast (Critical 1 件 = 10% 減点 / Normal 1 件 = 1% 減点 / 100% で fail fast)]` (`cmd/bench/main.go:204`、`scenario.go:310-314`)
- `[慣習らしい: 永続性検査は portal の再起動追試に委譲]` (bench 内に再起動後の検査が無い)
- `[慣習らしい: scoring は ScoreTag × 倍率の線形加点 (`step.AddScore(tag)` × 倍率 map `ResultScoreMap`)]` (`tags.go:41-56`、`cmd/bench/main.go:268-276`)
- `[慣習らしい: portal 連携は protobuf BenchmarkResult を `ISUXBENCH_REPORT_FD` に書き戻す (`benchrun.NewReporter`)]` (`cmd/bench/main.go` の portal 連携部)
- `[慣習らしい: 初期データ + 期待値を bench に同梱 (JSON dump、`benchmarker.json` / `benchmarker_tenant.json`)、データ生成器も同 repo (`isucon12-qualify/data/`) に持つ]`
- `[慣習らしい: load の並列度を `worker.WithInfinityLoop` + `WithMaxParallelism(N)` の 2 段で動的にコントロール]`
- `[この回特有: ContestantLogger と AdminLogger を分けてエラー詳細とスタックトレースを使い分け]` (`logger.go:8-12`、`cmd/bench/main.go:167-176`)
- `[この回特有: ValidationError + ResponseValidator 高階関数 (WithStatusCode / WithSuccessResponse[T] / WithContentType / WithCacheControlPrivate)]` (`validation.go:73-227`)
- `[この回特有: `RequestWithRetry` で 429 + Retry-After を吸収するのは更新系 (テナント追加・大会追加) のみ]` (`action.go:31-46, 101-119, 221-266`)
- `[この回特有: マルチテナントを SQLite ファイル単離で表現し、bench も「テナント DB ごと」を意識した負荷生成 (NewTenant → OrganizerJob → PlayerScenario の連鎖) を行う]` (`isuports.go:74-95`、`scenario_new_tenant.go` 全体)
- `[この回特有: 初期データの tenant ID 範囲を 4 区画に切って Validate シナリオ間の排他を取る]` (`constants.go:24-31`)
- `[この回特有: 3 サブドメイン (`admin.` / `{tenant}.` / 共通) + RSA JWT cookie + Host header 一致検証]` (`action.go:46-57`、`isuports.go:235-318`)
- `[この回特有: `-reproduce` で当日バグ込みの PlayerScenario を再現できる]` (`scenario_player.go:203`)
- `[この回特有: PlayerScenario が「1.2s × 3 で離脱」モデルを採用、減点ではなく加点機会の損失で罰する]` (`scenario_player.go:131-136`)
- `[この回特有: `staticFileCheck` で SPA バンドル (`/index.html` + js/css 各 1 ファイル) のバイト一致を検証]` (`scenario_validation.go:133-186`)
- `[この回特有: `BillingReport` を `cmp.Diff` で完全一致検証]` (`scenario_tenant_billing_validate.go:259`)
- `[この回特有: 走行中も Validate worker を常駐させて整合性検査を継続]`

## 6. 実装の不具合・残課題 (事実列挙)

bench code 内に観察された事実のみ。manual 文言との差は含まない。

1. **初期データロードが 2 回実行されている**: `scenario.go:151` と `scenario.go:168` で `sc.InitialData, err = GetInitialData()` が同一処理で重複している (1 回目の結果が即座に上書きされる)。
2. **`ConstMaxError` / `ConstMaxCriticalError` が宣言されているが未使用**: `constants.go:4-5` で定義されているが、grep しても他の参照が無い (実際の閾値判定は `scenario.go:23` の `MaxErrors = 30` と `cmd/bench/main.go:204` のリテラルで行われている)。dead constant。
3. **`playerDisplayNames` の slice 初期化が二重カウントを引き起こす**: 3 箇所で `make([]string, N)` の後に `for i := 0; i < N; i++ { append(...) }` しているため、結果は長さ `2N` で先頭 N 要素が空文字列になる:
   - `scenario_new_tenant.go:84-87` (`addPlayerNum`)
   - `scenario_popular_tenant.go:107-110` (`addPlayerNum`)
   - `scenario_tenant_billing_validate.go:85-88` (`playerNum`)
   サーバー側がこれをどう扱うかは未確認だが、bench から見ると「想定の 2 倍の display_name (うち半分は空) を送っている」ことになる。
4. **同一の `if !ok` チェックが連続している**: `scenario_validation.go:1495-1501` で `initialTenant, ok := tenantIDMap[index]` の直後に `if !ok` を 2 回続けて書いている (2 つ目は dead branch)。
5. **`scenario_player_validate.go:427` に `// TODO` だけ残った空コメント**: `func(r ResponseAPICompetitions) error { // TODO _ = r; return nil }` で大会一覧のレスポンス検証が未実装のまま放置。
6. **`cmd/bench/main.go:246` の `Language: "galaxy"` ハードコード**: `// TODO /initialize で取得した言語を入れる` のコメント付きで、`Prepare` 内で取得した `lang` 変数が portal 報告に流れていない (`scenario.go:182` で取得は成功するがローカル変数で消える)。portal 上は実装言語が常に "galaxy" として記録される。
7. **`sc.kickedWorkerPlayerIDMap` への書き込みが goroutine 内で行われ、check が同期 lock のみで書き込みが直前に完了している保証がない**: `scenario.go:430-441` で `playerWorkerKick` が `go func() { ... lock; map write }()` する一方、`job_organizer.go:219` の `checkPlayerWorkerKicked` が同 lock を取って読むため、新規 worker 起動と check が短時間に競合した場合に同一 (tenant, player) で複数の `PlayerScenarioWorker` が立ち上がる可能性がある。
8. **`cmd/bench/main.go:113` のコメント外し**: `// normalErrorCount := 0 // NOTE: validateErrorsを信頼する` で変数定義を消したが、:194 で再定義 `normalErrorCount := len(validateErrors) - criticalErrorCount` している。書き換え途中の痕跡。
9. **`scenario.go:286-287` のデバッグ用フィルタがコメントアウトのまま残っている**: `// if w.String() != "PlayerValidateScenarioWorker" { continue }` (該当 worker 以外を起動しない実験用 stub)。実害は無いが残置物。

## 7. この回固有の特殊事情

12q (ISUPORTS) はテナントごとに独立した SQLite ファイルを持つマルチテナント SaaS が題材で、bench 設計もこの特殊性を強く反映している。具体的には (a) 初期データの tenant ID 範囲を 4 区画に分けて Validate worker 同士の干渉を物理的に排除する設計、(b) AdminBilling 1 周完走 → 新規テナント追加 → そのテナント専用の NewTenant worker spawn → OrganizerJob 完了 → そのテナント専用の PlayerScenarioWorker spawn という連鎖で「テナント追加に応じて負荷ツリーが伸びる」動的増殖モデル、(c) 巨大テナント (id=1 `isucon`) を専用 `PopularTenantHeavyTenant` シナリオで遅れて 1 体だけ起動し、N+1 / 全件スキャンの暴発を test する仕掛け、の 3 点が際立つ。さらに `-reproduce` flag で「当日のバグ込み挙動 (ranking が空のとき sleep 無しで連打)」を切り替えられる仕組みが入っている (`scenario_player.go:203` `PlayerScenarioReproduce`、コメントに「以下は予選開催時の状態」「予選後のメモ」あり)。**推測:** 競技後 bench の修正と当日採点の再現性を両立する設計と読める。
