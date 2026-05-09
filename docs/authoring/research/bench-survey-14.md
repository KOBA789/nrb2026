# ISUCON 14 ベンチマーカー survey

## 0. メタ情報

- 競技日: 2024-12-08
- 言語 (一次): Go
- bench code root: `../isucon/isucon14/bench/`
  - 主要: `benchmarker/world/`, `benchmarker/scenario/`, `benchrun/`, `payment/`, `cmd/`
- webapp Go root: `../isucon/isucon14/webapp/go/`
- manual source:
  - `../isucon/isucon14/docs/manual.md` (当日マニュアル / 失格条件)
  - `../isucon/isucon14/docs/ISURIDE.md` (アプリケーションマニュアル)
- 補助参照:
  - `../isucon/isucon14/webapp/payment_mock/openapi.yaml` (= 決済 API 仕様、bench の `payment/` は同じ I/F の別実装)

## 1. 全体構造

ISURIDE は配車サービス (ride-hailing) を題材としたコンテストで、ベンチマーカーは「実世界をシミュレートして webapp に対して継続的にリクエストを送る」**世界モデル方式**を採用している。

### phase 構造

`isucandar.Benchmark` フレームワークで `Scenario.{Prepare, Load, Validation}` を順に呼ぶ (`bench/cmd/run.go:115-140`)。

- **Prepare** (`bench/benchmarker/scenario/scenario.go:181-219`)
  1. `validateFrontendFiles` (skipしうる、`-s` フラグ)
  2. `initializeData`: `POST /api/initialize` → 初期 Owner 5 / Owner あたり初期 Chair 4 / 初期 User 10 を作成
  3. `prevalidation`: `validateInitialData` (= 初期データに対する 4 セッションぶんの GET レスポンス完全一致検査)
- **Load** (`scenario.go:266-339`)
  - `World.Tick(worldCtx)` を 30 ms 周期で延々と呼び続ける (= 仮想世界 1 分 = 1 Tick)
  - `world.Tick` 内で全 Chair / User / Owner の `Tick` を goroutine で並列実行
  - `paymentErrChan` を毎周期 select し、payment server の検証エラー (`ErrorCodeWrongPaymentRequest` 等) を critical として即終了
  - `isucandar.WithLoadTimeout(60s)` で context が cancel される (`cmd/run.go:117`、manual の「負荷テスト 60 秒」と一致)
- **Validation** (`scenario/validation.go:13-42`)
  - 5 秒 sleep で payment server に届きうる飛び込みリクエストを待つ
  - `paymentServer.Close()` で payment server を `503` 化
  - 結果 (Score, pass) を最終 report

### scoring 概観

`Scenario.Score` (`scenario.go:345-360`):

- 加点: `Owner.SubScore` の合計を 100 で割った値 (Owner 単位で `Request.Score()` が積算されている、`world/user.go:201`)
- final 時のみ未評価リクエストの `PartialScore` を加算 (= load timeout 直後で評価未到達なものの partial 加点)
- 減点・FAIL の表現: `BenchmarkResult.Score` は raw のみ (`Deduction: 0` 固定、`scenario.go:380`)。失敗は `Passed: false` として表現

`Request.Score` (`world/request.go:192-194`): `売上 + 椅子の初期位置→PickupPoint の距離 * 10` (manual §「スコアの計算」と整合)。

### CLI flag / env

`bench/cmd/run.go:149-160`:

| flag | デフォルト | 役割 |
|---|---|---|
| `--target` | `http://localhost:8080` | webapp URL |
| `--addr` | `""` | webapp ip:port |
| `--payment-url` | `http://localhost:12345` | webapp に通知する payment server URL |
| `--payment-bind-port` | `12345` | bench 内蔵 payment server の bind port |
| `-t, --load-timeout` | `60` | 負荷走行秒数 (0 で prepare のみ) |
| `--fail-on-error` | `false` | エラー時に exit 1 |
| `--only-post-validation` | `false` | 再起動追試モード (= `validateInitialData` 単体実行) |
| `--metrics` | `false` | OpenTelemetry export |
| `-s, --skip-static-sanity-check` | `false` | frontend SHA 検査 skip |

env (`bench/benchrun/env.go`):

- `ISUXBENCH_TARGET` — supervisor (= portal) からの target 指定。セットされると `targetURL = "https://xiv.isucon.net"` に固定し `targetAddr = "<env>:443"` (`cmd/run.go:46-53`)
- `ISUXBENCH_ALL_ADDRESSES` — 取得関数のみ存在 (現コードで未使用)
- `ISUXBENCH_REPORT_FD` — protobuf `BenchmarkResult` を書き戻す pipe FD

### 入出力

- 入力: 上記 CLI / env のみ。問題データは `bench/benchmarker/scenario/data/` に embed (initial validation 用 4 セッションぶんの想定 JSON)、frontend hash は `bench/benchrun/frontend_hashes.json` に embed
- 出力: `BenchmarkResult` proto (`benchrun/gen/isuxportal/resources/`) を `ISUXBENCH_REPORT_FD` に長さ prefix 付きで書き出し (`benchrun/reporter.go:33-66`)。負荷走行中は 3 秒おきに中間 score を report (`scenario.go:271-287`)

## 2. ドメインと擁護する不変条件

### 2.1 ベンチが「絶対に守らせる」business invariant

critical 化されると 1 件で fail する (`world/errors.go:86-96`)。manual §「クリティカルエラー」に列挙されているもの (9 種) と完全対応している。

| 不変条件 | ErrorCode | 検出箇所 |
|---|---|---|
| ライドしていないユーザーに状態遷移通知が来てはいけない | `UserNotRequestingButStatusChanged` | `world/user.go:387-388` |
| アサインされていない椅子に状態遷移通知が来てはいけない | `ChairNotAssignedButStatusChanged` | `world/chair.go:457` |
| ユーザー側 / 椅子側で許容外の状態遷移が起きてはいけない | `UnexpectedUser/ChairRequestStatusTransitionOccurred` | `world/user.go:407-410`, `world/chair.go:463` |
| 椅子は完了通知前に別のライド通知を受け取ってはいけない | `ChairAlreadyHasRequest` | `world/chair.go:421-423` |
| マッチング待ちは 30 秒以内 | `MatchingTimeout` | `world/user.go:146-148` |
| 評価リクエストはタイムアウトしてはいけない | `EvaluateTimeout` | `world/user.go:179-181` |
| 評価完了したのに支払いが行われていないライドが存在してはいけない | `SkippedPaymentButEvaluated` | `world/user.go:190-192` |
| 決済サーバーへの支払い額・トークン・重複が誤っていてはいけない | `WrongPaymentRequest` | `world/payment.go:36-46` (bench 内 PaymentDB.Verify) |

検出メカニズム:

- 通知系の検証は SSE/JSON polling 受信側 (`scenario/worldclient/worldclient.go:243-317`) → 各 entity の `HandleNotification` で `ChangeRequestStatus` / `Validate*` を呼ぶ階層に集約
- 決済系の検証は webapp が叩く決済 API を bench 内同居 server (`bench/payment/`) で受け、`PaymentDB.Verify` (`world/payment.go:23-50`) が `Request.Paid.CompareAndSwap(false, true)` で重複検知 + `Request.Fare()` と一致検査
- critical の昇格は `World.handleTickError` (`world/world.go:461-475`) で `IsCriticalError` 真なら `criticalErrorCh` に流し込み、`World.Tick` の select が拾う (`world/world.go:154-168`) → `Scenario.Load` が `s.failed = true; return err` (`scenario.go:299-303`)

soft error は `ErrorCounter.Add` で計上され、合計 200 件超で「発生しているエラーが多すぎます」として上記 critical 経路に合流 (`world/errors.go:198-207`)。manual §「負荷走行の打ち切り」§「200 件以上のワーニング」と整合。

### 2.2 責務分担の境界線

| 担当 | 内容 |
|---|---|
| **bench** | 通知・ライド進行・座標・売上・nearby-chairs・初期データ完全一致・frontend SHA・決済整合 |
| **portal (job dispatch supervisor)** | bench プロセスへの起動 (env `ISUXBENCH_TARGET` / FD `ISUXBENCH_REPORT_FD` を渡す)、`BenchmarkResult.Execution` の終了理由・キュー管理 |
| **運営手動 (追試)** | 競技終了後の 3 台再起動、envcheck 実行確認、複数スタック VPC 検査、再起動による永続性確認 (= load 中追加データの再起動後到達性)、frontend 表示の人手確認 (manual §「追試手順」末尾 2 項) |
| **競技者** | matcher (`/api/internal/matching` 500 ms ポーリング) のチューニング、参考実装からの最適化 |

「bench がやらないこと」(= bench スコープ外) を明記する:

- **再起動後の永続性検査** — manual §「追試手順」最後から 2 番目に「負荷走行実行時にアプリケーションに書き込まれたデータが、再起動後に取得できるかどうかを確認します」と明記される。これは ISUCON 慣習として portal/運営追試に委譲。bench に再起動制御は入っていない。`--only-post-validation` はあくまで「初期データのみ」を再検査するモード (§3.3 参照)
- **frontend 表示の人手確認** — manual §「追試手順」最終項「アプリフロントエンドにアクセスし、表示を確認します」。bench は SHA 一致のみ確認 (`scenario/prevalidation.go:18-68`)
- **`/api/app/notification`, `/api/chair/notification` の SSE 形式の優先・JSON polling との切り替え** — どちらの実装でもよいことが ISURIDE.md §「通知エンドポイント」で明示。bench の SSE クライアントは両形式に対応 (`worldclient.go:252-310`)

## 3. 検査の構造

### 3.1 整合性検査

3 種に分かれる。

#### (a) フロントエンド静的ファイル検査

`Scenario.validateFrontendFiles` (`scenario/prevalidation.go:18-68`)。`benchrun/frontend_hashes.json` (build 時に embed) に対し、webapp の各 path (`/client`, `/owner`, その他 `frontend_hashes.json` 列挙のもの) の MD5 hash を比較。`/`, `/owner` は `index.html` と一致期待 (SPA fallback 対応)。`-s` フラグで skip 可能 (= 開発・トラブル時の運営オプション)。

#### (b) 初期データ整合性検査

`prevalidation` (`scenario/prevalidation.go:71-84`) → `validateInitialData` (`scenario/prevalidation.go:86-224`)。事前定義された 4 セッション (Owner 1 名 + User 3 名のクッキー固定) で以下を期待値と完全一致比較:

- `GET /api/owner/chairs` (Owner)
- `GET /api/owner/sales` (全期間 + 期間指定)
- `GET /api/app/rides` (User × 3)
- `POST /api/app/rides/estimated-fare` (User × 2、それぞれ 2 座標)

期待値は `bench/benchmarker/scenario/data/` 配下の embed JSON (`scenario/data.go`)。`go-cmp/cmp.Equal` + `cmpopts.SortSlices` で順序非依存比較。

#### (c) 負荷走行中の連続検査

各 entity の `Tick` 内で常時動作する。

- **`User.HandleNotification` 系** (`world/user.go:424-495`) — SSE/polling で受け取った通知の状態遷移許容セット (`ChangeRequestStatus`, `world/user.go:376-422`) と payload 内容 (`ValidateNotificationEvent`, `world/user.go:497-553`) をチェック。状態遷移には「許容される race」が `world/user.go:394-411` に明示的にハードコード (例: Dispatching を経ず Dispatched に飛ぶ、Dispatched から直接 Carrying を受ける、過去 ride への Completed)
- **`Chair.HandleNotification`** (`world/chair.go:417-477`) — 同様。Matched 通知時に直近の matchingData と異なる ServerRequestID が来たら critical
- **`Owner.Tick`** (`world/owner.go:70-127`) — 仮想世界 30 分ごとに `GET /api/owner/chairs`、毎時末に `GET /api/owner/sales` を叩き、bench 側の累積と比較 (`ValidateChairs` / `ValidateSales`)。売上一致は ±0 不要・期待 snapshot との range 内 (race 許容)
- **`User.CreateRequest` 内の nearby-chairs 検査** (`world/user.go:341-347` → `World.checkNearbyChairsResponse`, `world/world.go:326-459`) — 返却された椅子 ID が active か / 距離が範囲内か / 過去 3 秒以内に存在したか / 「3 秒前から動いていなくて範囲内にいる空車椅子が含まれていない」(suspicious chair) の 3 秒後遅延チェックまで含む高度な整合性検査
- **`User.CheckRequestHistory`** (`world/user.go:257-299`) — Active で進行中ライドが無い User が定期的に `GET /api/app/rides` を叩いて履歴一致を検証

通知の発火点は `worldclient.go:243-317` と対応する user 版 (= SSE/polling)。

### 3.2 負荷走行 (load)

`Scenario.Load` (`scenario.go:266-339`)。

- **Tick 周期**: 30 ms (`scenario.go:71`、`world.NewWorld(30*time.Millisecond, ...)`)
- **並列度**: 各 Tick で全 Chair / User / Owner ごとに goroutine 起動 (`world/world.go:123-152`)。Owner / Chair / User の数は時間と共に増加する
  - User: 仮想世界 60 分ごとに各 Region の `UserSatisfactionScore` に応じて増加 (`world/world.go:101-121`)、評価 4 以上のリクエスト完了で招待ユーザー追加 (`world/world.go:485-494`)
  - Chair: Owner ごとに売上に応じて指数的に増加 (`world/owner.go:108-123`、`desiredChairNum`)
  - Owner: 初期 5 名固定。途中で追加されない (`scenario.go:230-243`)
- **持続時間**: 60 秒 (`isucandar.WithLoadTimeout`、CLI `-t` で変更可)
- **バックオフ**: chair 座標送信 (`SendChairCoordinate`) のみ `backoff.NewExponentialBackOff` で成功までリトライ (`world/chair.go:307-322`)。それ以外の HTTP 失敗は soft error として ErrorCounter に積む
- **fail fast 条件**:
  - critical エラー 1 件で即終了 (`scenario.go:298-303`、`world/world.go:154-157`)
  - soft error 累計 200 件超で `errors.New("発生しているエラーが多すぎます")` → critical 経路 (`world/errors.go:203-205`)
  - Prepare 失敗時は Load に進まない (`isucandar` 仕様)
  - paymentErrChan からエラー受領で即終了 (`scenario.go:306-311`)
- **ハング設計**: 多くの状態 (Carrying 待ち、評価待ち等) は entity 側で「待つだけ」の break で表現され、状況が動かなければそのまま Tick が空回りする (`world/chair.go:177-237`、`world/user.go:142-216`)。manual の各 30 秒・15 秒タイムアウトのうち、ハードな critical 化は MatchingTimeout のみ
- **3 秒おきの中間 report**: `Scenario.Score(false)` を計算して portal に送信 (`scenario.go:271-287`)

### 3.3 最終チェック / 永続性検査の所在

`Scenario.Validation` (`scenario/validation.go:13-42`):

1. 5 秒 sleep — manual §「負荷テスト終了時点で行われていた決済処理は負荷テスト終了後 5 秒以内に完了する必要があります」と整合。この間に webapp からの遅れた決済が bench 内 payment server に届けば検査される
2. `paymentServer.Close()` で payment server を `503` 化 (= 5 秒越えのリクエストは success しない)
3. `sendResultWait.Wait()` で 3 秒中間 reporter の goroutine 終了待ち
4. `sendResult(s, true, !s.failed)` で final report

bench 内 final-check は実質「決済 5 秒猶予のみ」。それ以外の永続性・正当性検査は `--only-post-validation` モードと組み合わせて運用される。

`PostValidation` (`scenario/postvalidation.go:11-24`、CLI `--only-post-validation` で発火):

- `validateInitialData` を再走するのみ (= prevalidation と同じ embed 期待値で初期データ完全一致のみ確認)
- **load 中に追加された User / Chair / Owner / Ride 等は検査対象外** (= bench 内には永続化検査の発火点が無い)。これは load 中追加データの再起動後到達性が manual §「追試手順」末尾 2 番目で portal/運営追試の責務として明示されており、ISUCON 慣習として bench スコープ外であることと整合 (§2.2 参照)

## 4. 採用された手法 (再利用候補)

### 4.1 世界モデル方式 (= 仮想世界の能動シミュレーション)

`world/` 配下に Region / Owner / Chair / User / Request / Payment の能動オブジェクトを置き、各 entity の `Tick(ctx)` メソッドが「30 ms ごとの 1 分」のなかで自分の状態に応じた行動 (= 移動、ライド要求、評価、椅子の追加登録) を取る。bench は `World.Tick` を回すだけで、entity 自身が webapp に対する HTTP リクエストを生成する (`world/chair.go:89-325`、`world/user.go:95-246`)。

何を達成しているか:

- 全 entity が常に「次にやるべきこと」を持っているため、低負荷状態でも動的にトラフィックが生まれ、フロー制御が「シナリオ記述」ではなく「entity の状態機械」に分散する
- 通知整合・座標整合・売上整合がすべて「ベンチが知っている状態 vs サーバが返す状態」の比較に統一でき、整合性検査と負荷走行が分離しない
- ride の状態遷移が「許容される race」も含めてハードコードされた状態機械として表現される (`world/user.go:394-411`)

合同演習2026 で使えるか: ISUCON 14 が本格的世界モデル方式の最初の回 (= 9q / 12q / 12f / 13 はいずれも別方式) であり、設計コストは高いが、配車・取引・在庫など「状態遷移」中心のドメインなら適合する。Rust 単言語かつ作問物が小規模なら、より軽量な scenario-driven 方式 (例: 13 の客スレッド方式) も比較検討候補。

### 4.2 payment mock を bench プロセス同居

`payment/server.go:30-43` で `http.NewServeMux` を組み立て、`bench/cmd/run.go:153` の `--payment-bind-port=12345` で listen 開始 (`scenario.go:75-79`)。bench の起動と同時に payment 受信が始まり、Validation 終了時に `paymentServer.Close()` で 503 化 (`scenario/validation.go:16`)。

特徴:

- **`Idempotency-Key` 対応** (`payment/handler.go:36-46`): 同一キーでの再送は同じ `Payment` を返す。`p.locked` で重複処理を 1 つに絞る
- **確率的 5xx**: 「直近 3 秒で処理された payment 数 / 100」(最大 50%) の確率で 500/502/504 を返す (`payment/handler.go:71-127`)。ただしリトライ回数 ≥ 5 で必ず処理 (= 過剰リトライ対策)
- **検証はベンチの `World.PaymentDB.Verify` に委譲**: amount が `Request.Fare()` と一致しない / 重複決済 / トークン未登録なら critical (`world/payment.go:23-50`)

合同演習2026 で使えるか: 9q から続く「外部依存をベンチプロセスに同居させる」idiom がここまで成熟。Idempotency-Key + 確率的 5xx は「冪等性を実装させたい / リトライを実装させたい」課題で素直に流用できる。**注: 事実として参考実装 Go (`webapp/go/payment_gateway.go:30-94`) は `Idempotency-Key` を送信していない。ISURIDE.md §「Idempotency-Key ヘッダ」は「使うことができます」(can use) と記述。bench 側 (`payment/handler.go:36-46`) は `Idempotency-Key` の有無で挙動を分岐させるが、いずれの場合も fail させない**。

### 4.3 `--only-post-validation` モード (= 再起動追試の bench 側口)

CLI フラグで切り替え (`bench/cmd/run.go:92-111`)。Load を回さず `validateInitialData` のみ実行し、結果を `BenchmarkResult{Score: 0}` で portal に通知 (`Passed: true/false` のみ意味を持つ)。

何を達成しているか: portal 側の追試 workflow から「再起動後にも `POST /api/initialize` 後の初期データが正しく取れるか」だけを切り出して走らせるためのモード。本格的な再起動追試 (= load 中追加データの永続性) は portal/運営手動の責務であり、bench はあくまで初期データ完全一致の口を提供する。

合同演習2026 で使えるか: portal の job dispatch protocol で同じ binary の別モード起動を許せば、再起動追試で「初期化処理の正当性」だけを切り出して再検証できる。素直な idiom。

### 4.4 frontend 静的ファイルの SHA 埋め込み

`bench/benchrun/frontend_hashes.json` を `//go:embed` で binary に埋め込み (`benchrun/frontend_vaildator.go:14-15`)、prepare 時に各 path の MD5 と比較 (`scenario/prevalidation.go:30-66`)。SHA mismatch で「frontend を改変してはいけない」を bench 側から強制。

`-s` フラグで skip 可能 (= 開発時 / トラブル時の運営オプション)。

合同演習2026 で使えるか: 「frontend 改変禁止」を機械的に強制したい場合に直接流用可能。MD5 で十分。

### 4.5 確率的迂回 (= 椅子の振る舞いに「最適でない」要素を入れる)

`Chair.Tick` の Dispatching/Carrying で 10 % の確率で迂回ポイントを設定し、最短経路から外れた経路を取る (`world/chair.go:259-270`)。matching の優劣評価が「乗客の不満」として scoring に反映される (`Request.CalculateEvaluation`, `world/request.go:130-170`) ため、bench 側で「最適とは限らない」現実感を持たせる。

合同演習2026 で使えるか: 「最適化が即スコア最大化につながらない」「マッチングの上手さがスコアに反映される」設計を取りたいときに参考になる。ただし re-implement のコストは小さくない。

## 5. 設計上の選択点 (横断タグ)

- `[慣習らしい: pretest/prepare = 整合性検査専念 (FAIL fast)、load = 性能/scoring 専念、validation = 最終 close + report の責務分離]`
- `[慣習らしい: critical / soft (warning) の二段階エラー分類 + soft 件数閾値で critical 化または fail fast (本回は soft 200 件超で critical 化)]`
- `[慣習らしい: 永続性検査は portal の再起動追試に委譲 (bench 内に `--only-post-validation` モードあり、初期データ完全一致のみ)]`
- `[慣習らしい: 外部サービス (payment / shipment) mock を bench プロセス内同居 + bench から内部状態を直接 inspect]` — 決済 mock を bench プロセス同居 + 検証は bench 内構造から行う。
- `[慣習らしい: portal 連携は protobuf BenchmarkResult を `ISUXBENCH_REPORT_FD` に書き戻す (`benchrun.NewReporter`)]`
- `[慣習らしい: scoring は ScoreTag × 倍率の線形加点 (本回は raw のみ、Deduction 0 固定)]`
- `[慣習らしい: load の並列度を成功カウンタや競技者操作で動的に引き上げる (本回は entity 数が動的に増える)]`
- `[この回特有: 世界モデル方式 (= 各 entity が自律的に Tick で行動する setup)]` — 9q / 12q / 12f / 13 はいずれも別方式 (13 は `StatsSched` で前夜形)。
- `[この回特有: 30 ms 周期で全 entity が並列 goroutine で動く tick ベース設計]`
- `[この回特有: frontend SHA を bench に embed して静的ファイル改変を機械的に禁止]`
- `[この回特有: payment mock が Idempotency-Key 対応 + 確率的 5xx (失敗率は直近処理数依存、最大 50 %)]`
- `[この回特有: nearby-chairs 検査の suspicious chair 機構 (3 秒後の延期チェック) で「ベンチが認識する空車」と「サーバが認識する空車」の race を許容しつつ最終的に検査する]`
- `[この回特有: ride 状態遷移の「許容される race」をハードコードで列挙 (Dispatching skip、Dispatched→Carrying、過去 ride への Completed 通知)]`
- `[この回特有: `--only-post-validation` モードを bench に内蔵して再起動追試の初期データ検査を切り出せる設計]`
- `[この回特有: SSE / JSON polling の両対応を bench 側で持つ]`

## 6. 実装の不具合・残課題 (事実列挙のみ)

code 内の事実のみ。manual 文言との差は含めない。

1. **`world/payment.go:28` の常に false な条件式**:
   ```go
   if p.Amount <= 0 && p.Amount > 1_000_000 {
       return payment.Status{Type: payment.StatusInvalidAmount, Err: nil}
   }
   ```
   `&&` で接続されているため、`Amount` が同時に「0 以下」かつ「1,000,000 超」となることはなく、この条件式は数学的に常に false。`StatusInvalidAmount` を返す経路は事実として実行不能。実際の amount 不一致検出は下の `req.Fare()` 比較で行われている

2. **`world/owner.go:243-246` のコメントアウトされた active 状態検査**:
   ```go
   // アクティブ状態の検査はリクエストのタイミングでズレることがあるので、検査しない
   //if (data.Active && chair.State != ChairStateActive) || (!data.Active && chair.State != ChairStateInactive) {
   //	return fmt.Errorf("activeが一致しないデータがあります (id: %s, got: %v, want: %v)", chair.ServerID, data.Active, !data.Active)
   //}
   ```
   `Owner.ValidateChairs` が返す `is_active` の整合検査が race 回避のためコメントアウトされたまま稼働している。コメント通り「タイミングでズレる」ことに対する代替検査 (例: 直近 3 秒以内ならズレを許容) は入っていない

3. **`scenario/prevalidation.go:226` の `validateSuccessFlow` (dead code)**:
   - 大規模な「register → ride 1 周フロー」の synchronous 検査関数 (`prevalidation.go:226-608`) が定義されているが、bench 内のどこからも呼び出されていない (`grep -rn validateSuccessFlow` で唯一の hit が定義のみ)

4. **`scenario/postvalidation.go:19` の `slog.String` の呼び方が捨てられている**:
   ```go
   if err := validateInitialData(ctx, clientConfig); err != nil {
       slog.String("初期データのバリデーションに失敗", err.Error())
       return err
   }
   ```
   `slog.String(key, value)` は `Attr` を作るだけのコンストラクタで、戻り値は捨てられている。log 出力関数 (`slog.Error` / `slog.Info` 等) は呼ばれておらず、結果として何も log 出力されないまま return している

5. **`benchrun/frontend_vaildator.go` のファイル名タイポ**: `vaildator` (= `validator` のタイポ) のまま稼働

## 7. この回固有の特殊事情

ISUCON 14 は本格的な「世界モデル方式」を採用している (**推測:** 他の対象回 9q / 12q / 12f / 13 と比較した観察として、本格的世界モデル方式の最初の回。最終判定は §5 タグ整合 review pass に委ねる)。9q / 12q / 12f / 13 のシナリオ駆動 (= テストケースを順に流す) や客スレッド型 (= 客 1 人ぶんの一連の挙動を 1 goroutine が回す) と異なり、Region / Owner / Chair / User / Request / Payment の能動オブジェクトが 30 ms 周期で並列 Tick し、自分の状態と環境に応じた次の行動を生成する。この方式は「配車サービスの実時間進行」を bench 内に閉じて表現できる代わりに、設計・実装・debug のコストが大きく、本 survey で観察された軽微なバグ (§6 の 1, 2, 4) や dead code (§6 の 3) はその規模感を反映している。

合同演習2026 への示唆: Rust 単言語 + 新規問題で世界モデル方式を採用するかは、ドメインが「複数の能動 entity が時間軸上で相互作用する」ものに当てはまるかで判断する。当てはまらなければ、9q 系のシナリオ駆動か 13 系の客スレッド型のほうが設計・実装ともに軽い。当てはまる場合は、本 survey §4.1〜§4.5 の idiom 群 (世界モデル / payment mock 同居 / `--only-post-validation` モード / frontend SHA embed / 確率的迂回) はそのまま参考にできる。
