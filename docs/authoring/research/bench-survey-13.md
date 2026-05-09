# ISUCON 13 ベンチマーカー survey

> 反映先: `docs/authoring/norms.md` § 3 (synthesis)

## 0. メタ情報

- **競技日**: 2023-11-25
- **言語 (一次)**: Go
- **bench code root**: `../isucon/isucon13/bench/`
- **webapp Go root**: `../isucon/isucon13/webapp/go/` (`main.go` + `*_handler.go` 7 種: livecomment / livestream / payment / reaction / stats / top / user)
- **manual source (3 文書併読)**:
  - **gist manual** (= ISUPipe アプリケーションマニュアル、API 仕様の正本): <https://gist.github.com/kazeburo/70b352e6d51969b214f919bcf0794ba6>
  - **specification.md** (= 問題コンセプト ドラフト、業務文脈の正本): `../isucon/isucon13/docs/specification.md`
  - **cautionary_note.md** (= 当日マニュアル、競技進行 / 失格条件 / DNS / スコア計算式の正本): `../isucon/isucon13/docs/cautionary_note.md`
- **補助参照**:
  - `../isucon/isucon13/docs/isupipe.yaml` (OpenAPI、ハンドラ I/O の補強)
  - `../isucon/isucon13/bench/Makefile` (build / 実行手順)

## 1. 全体構造

### 1.1 phase 構造

`cmd/bench/bench.go:147-353` の `run` action がエントリ。実行順序:

1. **CLI / config 読込** (`bench.go:90-194`): flags をパースし、`config.TargetWebapps` (許可 IP リスト) を `--nameserver` + `--webapp` から構築。`--enable-ssl` で `https` / port 443 に切替。
2. **初期化**: `isupipe.NewClient` でクライアント作成 → `client.Initialize(ctx)` で `POST /api/initialize` を 42 秒タイムアウトで叩く (`bench.go:201-222`、`InitializeAgentTimeout = 42 * time.Second` at `config/benchmark.go:27`)。レスポンスから `lang` を取得し `config.Language` に格納。
3. **Pretest フェーズ** (`bench.go:224-234`): `scenario.Pretest(ctx, contestantLogger, pretestDNSResolver)` を実行。20 秒タイムアウト (`PretestTimeout = 20 * time.Second` at `config/pretest.go:5`)。失敗で即 fail。
4. **Benchmark (load) フェーズ** (`bench.go:241-264`): `benchmarker.run(benchCtx)` を 60 秒タイムアウトで実行 (`DefaultBenchmarkTimeout = 60 * time.Second` at `config/benchmark.go:6`)。`--pretest-only` 指定時はここをスキップ (`bench.go:236-239`)。
5. **Finalcheck フェーズ** (`bench.go:266-273`): `scenario.FinalcheckScenario(ctx, ..., finalcheckDNSResolver)` を 10 秒タイムアウトで実行 (`FinalcheckTimeout = 10 * time.Second` at `config/finalcheck.go:5`)。**実装は事実上 stub** (詳細 §3.3、§6)。
6. **集計と JSON 出力** (`bench.go:276-352`): `bencherror.GetFinalBenchErrors()` でエラー文字列を集約し、`benchscore.GetTotalProfit()` を score として `BenchResult` 構造体 (`bench.go:34-40`) を `--result-path` に書き出しつつ stdout にも出力。

### 1.2 scoring

- **加点モデル**: 投げ銭合計 (Tip) のみ。`benchscore.AddTip(uint64)` (`internal/benchscore/profit.go:9`) が `client.PostLivecomment` 成功時に呼ばれる (`isupipe/client_livecomment.go:249`)。
- **減点モデル**: 存在しない (= スコア計算式は単純加算)。
- **FAIL 条件**: 初期化失敗 / Pretest 失敗 / load 中の goroutine が `bencherror.Done()` 後に集計される `Violation` カウントに該当 — ただし load 中に違反を即時検出する `RunViolationChecker` は `benchmarker.go:356` でコメントアウト (詳細 §6)。
- cautionary_note.md:454-458 の「スコアは投げ銭(Tip)機能による送金額の合計」「サーバとベンチマーカーで差分があればベンチマーカー値を採用」と一致。

### 1.3 CLI / env / 設定可能項目

`cmd/bench/bench.go:90-145`:

| flag | env | default | 用途 |
|---|---|---|---|
| `--target` | `BENCH_TARGET_URL` | `http://pipe.u.isucon.dev:8080` | webapp ベース URL |
| `--nameserver` | `BENCH_NAMESERVER` | `127.0.0.1` | DNS サーバ IP |
| `--webapp` (複数) | — | `[]` | 名前解決結果 IP の許可リスト (= server list) |
| `--dns-port` | `BENCH_DNS_PORT` | `53` | DNS ポート |
| `--assetdir` | `BENCH_ASSETDIR` | `assets/testdata` | アセットディレクトリ |
| `--staff-log-path` | `BENCH_STAFF_LOG_PATH` | `/tmp/staff.log` | 運営ログ |
| `--contestant-log-path` | `BENCH_CONTESTANT_LOG_PATH` | `/tmp/contestant.log` | 選手ログ |
| `--result-path` | `BENCH_RESULT_PATH` | `/tmp/result.json` | スコア JSON 出力先 |
| `--enable-ssl` | `BENCH_ENABLE_SSL` | `false` | HTTPS / port 443 化 |
| `--pretest-only` | `BENCH_PRETEST_ONLY` | `false` | Pretest 後に終了 (= load を skip) |

`supervise` サブコマンド (`cmd/bench/supervise.go`、`cmd/bench/sqs.go`、`cmd/bench/s3.go`、`cmd/bench/reboot.go`) は portal / SQS / S3 と連携する supervisor mode。本 survey の主対象は `run` サブコマンド。

### 1.4 入出力 (BenchResult JSON)

`BenchResult` (`bench.go:34-40`):

```go
type BenchResult struct {
    Pass          bool     `json:"pass"`
    Score         int64    `json:"score"`
    Messages      []string `json:"messages"`
    Language      string   `json:"language"`
    ResolvedCount int64    `json:"resolved_count"`
}
```

`ResolvedCount` は名前解決成功数 (`benchscore.GetByTag(benchscore.DNSResolve)`、`bench.go:326`)。これは「さくらインターネット企業賞」(cautionary_note.md:464-466 の「名前解決成功数」が最も多いチーム) の集計用にレポートされる。

## 2. ドメインと擁護する不変条件

### 2.1 ベンチが「絶対に守らせる」business invariant

bench が critical 化する不変条件 (= 1 件で fail に持ち込む条件):

- **(I-a) DNS 解決の許可 IP 制約**: `resolver.DNSResolver.Lookup` (`internal/resolver/dns.go:101-109`) は名前解決結果が `config.IsWebappIP(ip)` (`config/webapp.go:16-23`、= `--webapp` で渡された server list) に含まれない場合、`「%s」はサーバーリストに含まれていません` で失敗を返す。pretest の `dnsRecordPretest` (`scenario/core_pretest_dnsrecord.go:14-36`) で初期 16 個ぶん検証され、load 中も全リクエストの DNS 解決経路で同じチェックが効く。
- **(I-b) `pipe` ユーザの登録拒否**: `assertPipeUserRegistration` (`scenario/core_pretest_abnormal.go:20-44`) が `name=pipe` の登録に対し 400 を要求。`bench はこれを「'pipe'ユーザの作成は拒否されなければなりません」` で fail にする。`pipe.u.isucon.dev` は webapp 自体の host name に予約されているため。
- **(I-c) 不正ログインの拒否**: `assertBadLogin` (`scenario/core_pretest_abnormal.go:46-83`) が「存在しないユーザ」「パスワード違い」両方で 401 を要求し、`bencherror.NewViolationError` を発行する。これは bench 内で唯一明示的に `ViolationError` (= critical) を立てる箇所。
- **(I-d) ユーザ name の UNIQUE 制約**: `assertUserUniqueConstraint` (`scenario/core_pretest_abnormal.go:85-113`) が同じ name の 2 回目の `Register` で 500 を要求。
- **(I-e) 配信予約枠 overflow**: `assertReserveOverflowPretest` (`scenario/core_pretest_abnormal.go:115-179`) が `config.NumSlots = 5` (`config/reservation.go:9`) を超えて予約しようとしたとき、ループ中で 1 度でも失敗が返ることを要求。
- **(I-f) 配信予約期間外**: `assertReserveOutOfTerm` (`scenario/core_pretest_abnormal.go:181-232`) が 2026 / 2022 の StartAt に対し 400 を要求。
- **(I-g) Moderate 後のスパム除去 / NG ワード追加投稿拒否**: `NormalModerateLivecommentPretest` (`scenario/core_pretest_normal.go:822-969`) が moderate 後の `GetLivecomments` 件数差 `-1`、コメント文字列に NG word 非含有、再投稿時のエラーを要求する。
- **(I-h) アイコン未設定時の NoImage.jpg と SHA256 ハッシュ**: `NormalIconPretest` (`scenario/core_pretest_normal.go:472-573`) が `GET /api/user/:username/icon` で `NoImage.jpg` の bytes と sha256 hash が返ることを要求。
- **(I-i) アイコン更新の 2 秒以内反映**: 同 pretest 内で `IconHashAppliedDelay = 2 * time.Second` (`core_pretest_normal.go:28`) sleep 後に `icon_hash` が更新後ハッシュと一致することを要求。gist manual 「アイコン更新後 2 秒以内に反映」と一致。
- **(I-j) 統計値の正確性**: `normalStatsCalcPretest` (`scenario/core_pretest_calc.go:22-246`) が bench 内 `StatsSched` (世界モデルの初期段階に近い実装) と webapp の `GET /api/user/:username/statistics` / `GET /api/livestream/:livestream_id/statistics` を突合 (rank、favorite emoji、total reactions、max tip、viewers count、total reports)。
- **(I-k) 初期売上 0**: `normalInitialPaymentPretest` (`scenario/core_pretest_initial.go:16-37`) が `GET /api/payment` で `total_tip == 0` を要求。

### 2.2 責務分担の境界線

| 責務 | 担当 |
|---|---|
| 整合性の決定論的検査 | bench (Pretest フェーズ、§3.1) |
| 負荷生成と Tip 集計 | bench (load 6 シナリオ、§3.2) |
| DNS allowlist の常時検査 | bench (`resolver.Lookup` 内、全 HTTP リクエストの DNS 解決経路で常時動作) |
| DNS 水責め攻撃 | bench (`internal/attacker/dns.go`、§4 idiom 候補) |
| 永続性検査 (再起動後の負荷走行 + データ可読性) | **portal** (cautionary_note.md:474-484、再起動後負荷走行 + 上位チーム向け永続性追試) |
| 75% 再現スコア閾値 | **portal** (cautionary_note.md:478-479) |
| サーバ環境確認 (envcheck) | **運営手動 / 自動** (cautionary_note.md:212-213、`envcheck` コマンド) |
| 複数スタック混在の検出 | **運営手動** (cautionary_note.md:191-194) |
| ブラウザでの表示確認 | **競技者** (cautionary_note.md:138-142、「負荷走行中はブラウザでの表示は行わないのが推奨」) |
| HLS メディア配信の品質 | **対象外** (cautionary_note.md:234-238、`media.xiii.isucon.dev` は最適化対象外で bench 採点外) |
| moderate された spam の log 1 件除外 | gist manual で「採点除外」と明記 — bench 側でも `viewer_spam` シナリオが moderate 済 spam を 400 期待で投げる (`scenario/viewer.go:174-180`) ことで、log は出るが減点に影響しない |

bench がアクセスしない API: `GET /api/livestream/:livestream_id/ngwords` は `VisitLivestreamAdmin` (`scenario/visit_page.go:95-98`)、`BasicStreamerModerateScenario` (`scenario/streamer.go:114-168`)、`NormalModerateLivecommentPretest` (`core_pretest_normal.go:856`) で叩かれる。`GET /api/livestream/:livestream_id/reaction` は load 内 `BasicViewerScenario` (`scenario/viewer.go:125`) で叩かれる。`POST /api/livestream/:livestream_id/enter` / `DELETE /api/livestream/:livestream_id/exit` は `VisitLivestream` / `LeaveFromLivestream` (`scenario/visit_page.go:45-83`) で叩かれる。= router 定義されたエンドポイント (`webapp/go/main.go:132-183`) は基本的に bench から叩かれている。

## 3. 検査の構造

### 3.1 整合性検査 (Pretest)

`scenario.Pretest` (`scenario/core_pretest.go:66-130`) は以下 13 関数を**逐次実行**する。1 件でも失敗で即 fail。

| # | 関数 | ファイル | 検査対象 |
|---|---|---|---|
| 1 | `dnsRecordPretest` | `core_pretest_dnsrecord.go:14-36` | 初期 DNS レコード (`pipe.u.isucon.dev` + `DefaultDNSRecord` 16 個から random 10 + 存在しない名前 3 個で `IsWebappIP` 違反だけ拾う) |
| 2 | `normalInitialPaymentPretest` | `core_pretest_initial.go:16-37` | 初期売上 0 |
| 3 | `normalStatsCalcPretest` | `core_pretest_calc.go:22-246` | ユーザ統計 / 配信統計の前後比較 (世界モデル `StatsSched` との突合) |
| 4 | `NormalLivestreamPretest` | `core_pretest_normal.go:137-470` | tag 一覧 / livestream 予約 / 検索 / enter/exit (前後で 2 + 19 = 21 件予約しタグ別検索結果と件数突合) |
| 5 | `NormalUserPretest` | `core_pretest_normal.go:32-92` | register / login / GetMe / GetUser / GetStreamerTheme |
| 6 | `NormalIconPretest` | `core_pretest_normal.go:472-573` | NoImage.jpg / icon_hash / 2 秒反映遅延 / If-None-Match (etag) |
| 7 | `NormalReactionPretest` | `core_pretest_normal.go:663-724` | リアクション投稿 / GetReactions の差分 +1 |
| 8 | `NormalPostLivecommentPretest` | `core_pretest_normal.go:575-661` | livecomment 投稿 / icon_hash 反映 / report |
| 9 | `NormalModerateLivecommentPretest` | `core_pretest_normal.go:822-969` | NG word 登録 → 過去 spam の削除 / 再投稿の拒否 |
| 10 | `assertBadLogin` | `core_pretest_abnormal.go:46-83` | 不正ログイン (401) — `ViolationError` を立てる |
| 11 | `assertPipeUserRegistration` | `core_pretest_abnormal.go:20-44` | `pipe` ユーザ登録拒否 |
| 12 | `assertUserUniqueConstraint` | `core_pretest_abnormal.go:85-113` | name の UNIQUE 制約 (500) |
| 13 | `assertReserveOverflowPretest` | `core_pretest_abnormal.go:115-179` | 予約枠 overflow (NumSlots=5 のループで 1 件失敗) |
| 14 | `assertReserveOutOfTerm` | `core_pretest_abnormal.go:181-232` | 期間外予約 (400) |

(`assertMultipleEnterLivestream` (`core_pretest_abnormal.go:234-236`) はリストされているが空関数。)

検査スタイルは「決定論的に固定 user / 固定 livestream を使い、API のレスポンスを expected 値と直接比較」が基本。`StatsSched` (`internal/scheduler/stats_scheduler.go`) は bench 側で「初期 SQL とそれに対する pretest の操作」を再現するインメモリ世界モデルで、これと webapp の statistics endpoint の戻り値を突合する設計。

### 3.2 負荷走行 (load)

`benchmarker.run` (`cmd/bench/benchmarker.go:345-418`) が走らせる 6 シナリオ:

| シナリオ | 関数 | 並列度 (semaphore weight) | 特徴 |
|---|---|---|---|
| `loadStreamer` | `BasicStreamerColdReserveScenario` (`scenario/streamer.go:26-101`) | `BaseParallelism = 1` | 配信者が cold 予約 (short / long 半々)、10% でアイコン変更 |
| `loadModerator` | `BasicStreamerModerateScenario` (`scenario/streamer.go:114-168`) | 1 | streamer が livecomment_reports を取得し、含まれる NG word を抽出して `Moderate` |
| `loadViewer` | `BasicViewerScenario` (`scenario/viewer.go:21-147`) | `1 * 10 = 10` (「視聴者は配信者の 10 倍」) | 視聴開始 → 1 時間ごとに livecomment + tip 投稿 + reaction → exit。Tip 加点はここ |
| `loadViewerReport` | `BasicViewerReportScenario` (`scenario/viewer.go:201-224`) | 1 (実行前に `time.Sleep(1s)`、`benchmarker.go:306`) | spam pool の livecomment を report |
| `loadSpammer` | `ViewerSpamScenario` + `AggressiveStreamerModerateScenario` (`scenario/viewer.go:149-199` / `scenario/streamer.go:175-211`) を内部で goroutine 並列起動 | `1 * 2 = 2` (「視聴者の 2 倍 spammer」) | spam 投稿 (moderated なら 400 期待 / 非 moderated なら spam pool に積む) と aggressive な NG word 登録 |
| `loadAttack` | `DnsWaterTortureAttackScenario` (`scenario/attacker.go:13-23`) | 動的 (初期 `512/2 = 256`、`attackParallelis` を最大 15 まで上げる) | DNS 水責め攻撃 + 解決成功 IP に対して 5 回に 1 回 HTTPS GET |

並列度の動的調整: `loadAttackCoordinator` (`benchmarker.go:242-266`) が 2 秒ごとに DNS 失敗率 < 1% かつ 1 worker あたり解決数 > 50 なら `attackParallelis` を 1.5 倍に上げる (上限 15)。**推測:** スコアではなく「DNS 攻撃に耐えられた量」を企業賞 (cautionary_note.md:464-466) に反映するための調整機構と読める。

login 並列性の保証: `runClientProviders` (`benchmarker.go:150-203`) で `streamerLoginCounter` / `viewerLoginCounter` がそれぞれ `NumMustTryLogins = 10` (`config/benchmark.go:21`) に達するまで block。**推測:** 「初期 register/login が 1 件もできずシナリオが何も走らない」状況を防ぐガードと読める。

fail fast 条件: 主に「ctx 終了」(`benchmarker.go:362-370`)。`violateCh` (`benchmarker.go:356`) は本来 `RunViolationChecker` (`internal/bencherror/error.go:117-135`) からの violation を即時 break する設計だが、コメントアウトされている (詳細 §6)。

バックオフ / リトライ: load 中の DNS リトライは `ResolveAttempts: 1` (`internal/resolver/dns.go:57`) で実質リトライなし (cautionary_note.md:354 「負荷走行中はリトライを行いません」と一致)。pretest / finalcheck 用 resolver は明示的に `ResolveAttempts = 10` に上書き (`bench.go:211`、`bench.go:268`)。

### 3.3 最終チェック / 永続性検査の所在

`scenario.FinalcheckScenario` (`scenario/core_finalcheck.go:14-33`):

```go
func FinalcheckScenario(ctx context.Context, contestantLogger *zap.Logger, dnsResolver *resolver.DNSResolver) error {
    client, err := isupipe.NewCustomResolverClient(...)
    if err != nil { return err }

    // FIXME: ライブコメント存在チェック
    _ = client

    if err := os.WriteFile(config.FinalcheckPath, []byte("{}"), os.ModePerm); err != nil {
        return err
    }
    return nil
}
```

実装は `client` を作るが捨てて、`/tmp/finalcheck.json` (`config/supervise.go:9`) に `{}` を書き出すだけ。`config.FinalcheckPath` は `cmd/bench/s3.go` 経由で supervise mode が S3 にアップロードする想定の出力先。**bench プロセス内での最終チェックは事実上 no-op**。

永続性検査の所在: cautionary_note.md:474-484 の通り、portal が以下を担う:

- 全チームに対して: 再起動後の負荷走行で fail / 再現スコアが最終スコアの 75% 以下 / envcheck 失敗
- 上位チームに対して: 「負荷走行実行時にアプリケーションに書き込まれたデータが、サーバー再起動後に取得できない場合」

bench は「load 中に書いた値が再起動後に読めるか」を直接検査せず、**portal の追試 (= 再起動後にもう一度 bench を走らせて再現スコア / pass を見る)** に委ねる設計。責務分担の境界線は §2.2 を参照。

## 4. 採用された手法 (再利用候補)

### 4.1 bench 内蔵 DNS リゾルバ (`internal/resolver/dns.go`)

`miekg/dns` を直接使った独自リゾルバ。OS の resolver を経由せず、`config.TargetNameserver:DNSPort` を直接叩いて A レコードのみ要求し、応答 IP が `config.IsWebappIP` に含まれるかをチェックする (`dns.go:101-109`)。LRU キャッシュ (10000 エントリ、TTL に従う) も自前で持つ (`dns.go:18-26、dns.go:111-118`)。

**達成事項**: (1) DNS allowlist を全 HTTP リクエストの dial 経路で強制、(2) OS resolver による解決プロトコル違いを排除、(3) 独自キャッシュによる pretest 高速化、(4) `ResolveAttempts` を pretest=10 / load=1 で切替 (cautionary_note.md「負荷走行中はリトライを行いません」と一致)。

**合同演習2026 で使えそうか**: `--nameserver` 機構の祖型として直接参考にできる。`net.Resolver` の Go 標準より、`miekg/dns` 自前構築のほうが allowlist 検査と TTL ハンドリングを自由に書けるため採用するべき。

### 4.2 DNS 水責め攻撃エンジン (`internal/attacker/dns.go`)

`DnsWaterTortureAttacker` (`attacker/dns.go:37-45`) が UDP 接続を 10 リクエストごとに張り直しつつ、ランダムなサブドメイン (`<random10-32 文字>0.<random>0.u.isucon.dev.`) で A レコードを問い合わせる。30 リクエストに 1 回はラベル数を 1〜3 個増やす。解決成功した IP に対し 5 回に 1 回 HTTPS GET を投げる (`attacker/dns.go:77-117`)。

**達成事項**: cautionary_note.md:358-364 「DNS水責め攻撃」の現実的な負荷再現。`bytebufferpool` と `unsafe.String` で文字列生成のアロケーションを抑え、`sync.Pool` で `dns.Msg` を使い回す高並列化。

**合同演習2026 で使えそうか**: 合同演習2026 が DNS を絡めた問題を出すか次第だが、「外部からの常時負荷ノイズを bench 内で生成する」パターンは payment mock や notification mock と並ぶ「世界の他の actor を bench が代弁する」設計の代表例として idiom 化する価値がある。

### 4.3 3 文書併読の manual 構成

ISUCON 13 の manual は **gist (API 仕様)** + **specification.md (問題コンセプト)** + **cautionary_note.md (当日マニュアル / 失格条件 / DNS 仕様 / スコア式)** の 3 文書に分割されている。bench 側はこの分割に応じて、

- API レスポンス検証 → gist 由来 (例: `IconHashAppliedDelay = 2 * time.Second` は gist の「2 秒以内反映」と直接対応)
- DNS allowlist / 5 回リトライ → cautionary_note.md 由来 (例: `ResolveAttempts` の pretest=10 / load=1 切替)
- スコア式 (Tip 合計のみ) → cautionary_note.md:454-458 由来

と参照先を切り替える。

**合同演習2026 で使えそうか**: 「manual を 1 ファイルに詰めると、API spec / 当日進行 / 失格条件が混在して読みにくい」課題への解として有効。ただし 3 つに分けると今回のように cautionary_note.md 内で「負荷テスト 60 秒 (l.428)」と「負荷テスト 20 秒 (l.441)」の内部不整合が起きる risk もある (本不整合は manual 側の問題で bench 実装は 60 秒で正しい)。合同演習2026 では 2 文書 (API spec / 当日マニュアル) 構成が無難だが、「複数文書の正本を分けて、bench コードコメントで参照先を明示する」アプローチ自体は idiom として再利用可能。

### 4.4 世界モデル (`StatsSched` / `LivecommentScheduler` / `ReservationSched`) の前夜形

`internal/scheduler/` 配下に `stats_scheduler.go` / `livecomment_scheduler.go` / `reservation_scheduler.go` / `user_scheduler.go` 等、bench 側で「webapp が持つべき状態」を再現するインメモリ表現を持つ。pretest はこれと webapp の statistics endpoint を突合する (`core_pretest_calc.go:64-243`)。

**達成事項**: 統計値の正確性検査を「決定論的な操作 → 期待値計算 → API レスポンス突合」の形で書ける。

**合同演習2026 で使えそうか**: 13 では Pretest 内のみの利用で load 中の statistics 突合は行われない (load = 加点専念の責務分離) が、後発の 14 / 合同演習2026 で常時並走する世界モデルへの発展経路がここに見える。「load 中も世界モデルと突合する」を選ぶか「load = 加点専念で final-check を厚くする」を選ぶかの分岐点。

### 4.5 Pretest 専用と load 専用で resolver attempts を切り替える

`bench.go:211` (`pretestDNSResolver.ResolveAttempts = 10`) と `bench.go:268` (`finalcheckDNSResolver.ResolveAttempts = 10`) で pretest / finalcheck 用には 10 回リトライ、load 中の `agent` が使う resolver は `NewDNSResolver()` のデフォルト 1 回のみ (`internal/resolver/dns.go:57`)。

**達成事項**: pretest / finalcheck は決定論性が要るのでリトライで揺らぎを吸収、load 中はリトライしないことで「DNS 障害が即 HTTP エラーに伝播する」テスト性を担保。

**合同演習2026 で使えそうか**: シンプルだが効く idiom。pretest と load で resolver / client / timeout を別個に持つ構造は採用するべき。

### 4.6 `--pretest-only` モード

`bench.go:236-239` で Pretest 完了直後に exit するモード。

**達成事項**: 開発時に「整合性だけ確認したい」「load を回す前に動くかだけ見たい」という用途で短時間で feedback ループを回せる。

**合同演習2026 で使えそうか**: 開発時 / 競技者の手元動作確認の両方で有効。採用するべき。

## 5. 設計上の選択点 (横断タグ)

- `[慣習らしい: pretest/prepare = 整合性検査専念、load = 性能/scoring 専念の責務分離]` — `Pretest` (`core_pretest.go:66-130`) は決定論的な 13 関数列、`benchmarker.run` (`benchmarker.go:345-418`) は scoring counter の更新のみ。両者の責務分離が明確。
- `[慣習らしい: 永続性検査は portal の再起動追試に委譲]` — `FinalcheckScenario` は事実上 no-op で、永続性は portal の reboot 追試 (cautionary_note.md:474-484) に委譲。
- `[慣習らしい: scoring は ScoreTag × 倍率の線形加点 (本回は Tip 合計のみで減点なし)]` — `benchscore/profit.go` は加算のみ、`TooSlow` / `TooManySpam` の score tag は `counter.go:14-15` で定義されているが本実装では一切インクリメントされない (= 設計上のフックは残るが未使用)。
- `[慣習らしい: 失敗系 (異常系) pretest を 4〜6 件入れて business invariant を critical 化する]` — `core_pretest_abnormal.go` の 5 関数。
- `[慣習らしい: load の並列度を成功カウンタや競技者操作で動的に引き上げる (本回は DNS 攻撃の `attackParallelis` で代替)]` — DNS 失敗率と解決数 avg を 2 秒間隔で見て負荷を 1.5 倍ずつ上げる (`benchmarker.go:242-266`)。**推測:** スコア向上ではなく「DNS 攻撃の耐性測定」を企業賞 (cautionary_note.md:464-466) に反映する設計と読める。
- `[この回特有: 統計値 (payment / profit / livestream stats) を bench 内インメモリ世界モデル (`StatsSched` 等) で集計し、webapp の値と突合する経路を持つ]` — `benchscore.AddTip` (`profit.go:9`) で bench 内集計、cautionary_note.md:454-458 で webapp との差分があれば bench 値採用。pretest 内の `normalStatsCalcPretest` でのみ使われ、load 中は使わない (= 14 で完成形を見せる世界モデル方式の前夜形)。
- `[この回特有: bench 内蔵 DNS リゾルバ + DNS 水責め攻撃の同居]` — bench が「DNS の検査者」と「DNS の攻撃者」を兼務する設計。`internal/resolver/dns.go` + `internal/attacker/dns.go` の対称性。
- `[この回特有: 3 文書 manual 構成 (gist + specification.md + cautionary_note.md)]` — API 仕様と問題コンセプトと当日マニュアルが分離。
- `[この回特有: `IconHashAppliedDelay = 2s` で manual 文言「2 秒以内反映」を許容遅延として直接 sleep する]` — `core_pretest_normal.go:28`、`:527`、`:632`。
- `[この回特有: 24 種類のシナリオ score tag (`benchmarker.go:24-38`) のうち、scoring に直接使われるのは BasicViewer の Tip 加算のみ。他のシナリオの success/fail カウントは contestant log への表示用]` — `bench.go:300-323` の集計ロジックで scenario ごとの成功/失敗回数を log するが、最終スコアには反映しない。
- `[この回特有: `MasterBenchScenario` 系の `Cold` / `Long` / `Hot` 命名規則の 3 軸設計 (`Long` / `Hot` 系は未実装で `Cold` 系のみ稼働)]` — `BasicStreamerColdReserveScenario` (実装あり) / `BasicLongStreamerScenario` (FIXME stub) / `BasicLongStreamerHotScenario` (FIXME stub) の関係性。`Cold` (= 既存配信者でない予約) と `Long` (= 10 時間超配信) と `Hot` (= 人気配信者衝突) の 3 軸が当初設計された。

## 6. 実装の不具合・残課題 (事実列挙のみ)

- `scenario/core_finalcheck.go:25` — `// FIXME: ライブコメント存在チェック` のコメントが残り、`_ = client` で生成した client を捨てて `{}` を書くだけの実装になっている。
- `cmd/bench/benchmarker.go:356` — `violateCh := make(chan error) // とめておく bencherror.RunViolationChecker(ctx)` で `RunViolationChecker(ctx)` 呼び出しがコメントアウトされたまま稼働。`violateCh` は declaration はあるが send 元がないため、`benchmarker.go:367` の `case err := <-violateCh:` は永久に発火しない。
- `scenario/streamer.go:22` — `BasicLongStreamerScenario` が `// FIXME: impl` で空 (return nil)。
- `scenario/streamer.go:110` — `BasicLongStreamerHotScenario` が `// FIXME: impl` で空 (return nil)。これら 2 関数は呼び出し元自体が無いため dead code。
- `scenario/core_pretest.go:73-74` — `normalInitialPaymentPretest` 周辺に `// FIXME: reactions, livecommentsは統計情報をもとにチェックする` / `// FIXME: ngwordsはライブ配信のIDをいくつか問い合わせ、存在することをチェックする` のコメント。初期データ pretest が「初期売上 0」しか見ておらず、reactions / livecomments / ngwords の初期件数検査は未実装。
- `scenario/visit_page.go:89` — `VisitLivestreamAdmin` 内に `// FIXME: 自分のライブストリーム一覧を取ってくる必要がある` のコメント。実装は `SearchLivestreams` (= 全配信検索) で代用しており、本来叩くべき `GET /api/livestream` (自分のライブ配信) には差し替えられていない。
- `cmd/bench/bench.go:196` — `// FIXME: アセット読み込み` でアセット検証が実装されておらず、`contestantLogger.Info("静的ファイルチェックを行います") / 完了しました")` のログだけ出る。
- `cmd/bench/supervise.go:23` — `// FIXME: SQSのメッセージサイズが最大で256KBなので、200KB程度までで打ち切るように` のコメント。supervise mode の SQS 送信ペイロードが 256KB を超える場合の対策が未実装。
- `internal/benchscore/profit.go:14` — `// FIXME: finalcheck後にprofitをスコアに加算しないと駄目` のコメント。`GetTotalProfit()` は load 中の累積を返すだけで、finalcheck 後の加算ステップは持たない (finalcheck の現状は §3.3 を参照)。
- `internal/benchscore/counter.go:14-15` — `TooSlow` / `TooManySpam` の score tag が定義されているが、`IncResolves` / `IncDNSFailed` のような increment 関数が無く、bench code 内のどこからも `counter.Add(TooSlow)` / `counter.Add(TooManySpam)` は呼ばれない (= 集計フックは用意されたが使われていない)。
- `cmd/bench/benchmarker.go:306` — `time.Sleep(1 * time.Second) // XXX: report回りすぎ抑止` の sleep。`loadViewerReport` 開始ごとに 1 秒待つことで report の頻度を間引く workaround 実装。
- `internal/benchscore/profit.go:7-11` — `profit uint64` グローバル変数を `atomic.AddUint64` で更新するが、`bench.go:227` / `:245` で `benchscore.InitCounter(ctx)` を 2 回呼ぶ際に `profit` は reset されない (`InitCounter` は `counter` のみ初期化)。pretest 内では `core_pretest_calc.go:166-174` で `client.PostLivecomment` を呼ぶ箇所がある。

## 7. この回固有の特殊事情

ISUCON 13 ISUPipe は「ライブ配信 + 投げ銭 + DNS」という 3 つの異種要素を組み合わせた問題。bench 設計上の特異点は (1) **bench 内に DNS の検査者と攻撃者が同居**する点、(2) **scoring が Tip 合計の単純加算のみ**で減点機構を持たない点、(3) **pretest と load の責務が完全分離** (pretest = 13 関数の決定論的検査 / load = 6 シナリオの加点専念) で世界モデル (`StatsSched`) は pretest 内のみで使う点、(4) **永続性検査と再現性検査が portal の reboot 追試に全面委譲**で bench の `FinalcheckScenario` は事実上 stub である点。これらは互いに整合した設計選択で、合同演習2026 が同様の問題を出すなら「世界モデルを load 中も並走させるか / pretest 専用に閉じるか」「永続性検査を bench 内に持つか / portal に委譲するか」の二軸で立ち位置を決める判断材料になる。
