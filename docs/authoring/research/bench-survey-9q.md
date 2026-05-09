# ISUCON 9 予選 ベンチマーカー survey

## 0. メタ情報

- 競技日: 2019-09-07 / 2019-09-08
- 言語 (一次): Go (`go.mod` で `go 1.23.0` 指定、本リポジトリ ../isucon/isucon9-qualify/go.mod:3 — 最近メンテされた古い回)
- bench code root:
  - `../isucon/isucon9-qualify/bench/`
  - `../isucon/isucon9-qualify/cmd/bench/main.go`
- webapp Go root: `../isucon/isucon9-qualify/webapp/go/`
- manual: `../isucon/isucon9-qualify/docs/manual.md` (repo 内 markdown)
- 補助参照:
  - `../isucon/isucon9-qualify/CLAUDE.md`
  - `../isucon/isucon9-qualify/webapp/docs/APPLICATION_SPEC.md`
  - `../isucon/isucon9-qualify/webapp/docs/EXTERNAL_SERVICE_SPEC.md`

## 1. 全体構造

### 1.1 phase 構造

`cmd/bench/main.go:44-235` がエントリポイント。実行は次の 4 phase の sequential 構成:

1. **initialize** (`main.go:101-118`, `scenario.Initialize`, 20s timeout) — `POST /initialize` を叩いて payment / shipment URL を webapp に通知し、レスポンスから `campaign` (還元率) と `language` を受け取る。エラーがあれば即終了。
2. **verify** (`main.go:120-138`, `scenario.Verify`) — 初期チェック。ここで critical / application エラーが 1 件でも出たら即 fail。`scenario/verify.go:15-366` に 9 個の verify scenario が並列実行される。
3. **validation** (`main.go:140-173`, `scenario.Validation`, deadline = `ExecutionSeconds (60s)`) — 本番。並列で `Check` (整合性検査) と `Load` (負荷) と (campaign が有効なら) `Campaign` を回す。critical 1 件で fail、application 10 件以上で fail。
4. **final check** (`main.go:175-225`, `scenario.FinalCheck`) — 1 秒 cooldown のあと bench 内蔵の payment mock の決済記録と webapp の `/reports.json` を突合してスコアを算出。

### 1.2 scoring の概観

`cmd/bench/main.go:175-225` および `scenario/scenario.go:96-143`:

- **加点**: payment mock 上で「`done` ステータスかつ webapp 側にも transaction_evidence が存在する」取引について `report.Price` を加算 (`scenario/scenario.go:132-135`)。
- **減点**: application エラー 1 件あたり 500 点 (`main.go:202`)。trivial (=timeout) は 200 件超で 100 件ごとに 5000 点 (`main.go:204-207`)。
- **fail の典型**:
  - critical エラー 1 件 (`main.go:160`、`fails/fails.go:11-12`)
  - application エラー 10 件以上 (`main.go:160`, `main.go:188`)
  - スコア最終値が 0 以下 (`main.go:214`)
  - verify phase で error 1 件以上 (`main.go:124-125`)
  - initialize の application エラー 1 件以上 / `/initialize` レスポンスタイムアウト 20s 超

### 1.3 CLI flag / env / 設定可能項目

`cmd/bench/main.go:44-76`:

- `-target-url` (default `http://127.0.0.1:8000`)
- `-target-host` (default `isucon9.catatsuy.org` — TLS SNI と Host header に使う)
- `-payment-url` (default `http://localhost:5555`)
- `-shipment-url` (default `http://localhost:7001`)
- `-payment-port` / `-shipment-port` (mock listen port)
- `-data-dir` (default `initial-data`)
- `-static-dir` (default `webapp/public/static`)
- `-allowed-ips` (mock サーバの IP 制限、競技サーバのみ通す)

env は使っていない。

### 1.4 入出力 (portal protocol / 出力形式)

stdout に 1 行 JSON (`Output` 構造体、`cmd/bench/main.go:21-27`):

```
{"pass": bool, "score": int64, "campaign": int, "language": string, "messages": []string}
```

ログは stderr (`log.SetFlags` を `init()` で設定、`main.go:40-42`)。portal とのやり取りは「bench を 1 プロセス起動 → stdout の JSON 1 行をパース」という単純な protocol。

## 2. ドメインと擁護する不変条件

### 2.1 ベンチが「絶対に守らせる」business invariant

critical エラー (1 件で fail) として明示的に `fails.ErrCritical` で立てられる条件は次の 3 件:

1. **多重決済の禁止** (`bench/server/payment.go:121-129`) — payment mock の `reportStore.Set` が同一 item_id への二度目の決済登録を検出した時点で critical。「購入は 1 商品 1 回」という業務不変条件を、bench プロセス内蔵の payment mock 側で擁護する設計。
2. **決済額の改ざん禁止** (`bench/server/payment.go:230-241`) — bench が `ForceSet` で予約した price と、webapp 経由で payment mock に届く price が一致しない場合に critical。「webapp が外部決済サービスに送る金額は購入時点の商品価格と等しい」という不変条件。
3. **売り切れ商品が複数人から購入されないこと** (`bench/scenario/campaign.go:230-232`) — 人気者出品シナリオで「真に購入できた buyer」が 2 人以上現れた瞬間に critical。同時購入競合下での排他制御の要請。

それ以外の検査はすべて `fails.ErrApplication` (10 件まで許容) で扱う。例えば「商品 status / transaction_evidence status / shipping status の整合性」「商品一覧の件数 / 並び順 / 重複」「画像 MD5」「カテゴリ ID 整合」などは critical ではなく application エラー。

`cmd/bench/main.go:91-93` のコメント `"想定外のエラーなのでcritical扱いにしておく"` と `bench/fails/fails.go:91-94` から、「`failure.MessageOf` が失敗するレベルの予期しないエラー」も critical 扱いになる。

### 2.2 責務分担の境界線

- **bench (本プログラム) の責務**:
  - `POST /initialize` → verify → load → final-check の 1 サイクル分の自動測定
  - 整合性 (商品一覧の order/件数/重複/カテゴリ整合/出品者整合)、画像 MD5、status 遷移の検査
  - 静的ファイル md5 の検査 (`bench/scenario/verify.go:322-363` — verify phase 内)
  - 取引完了売上の集計 (final-check で `/reports.json` と payment mock の records を突合)
  - **payment / shipment は bench プロセス内に同居** (`bench/server/server.go:114-144`、`cmd/bench/main.go:79-95`) — 本来「外部サービス」だが、bench プロセス内で起動して bench から状態を直接 inspect できるようにしている
- **portal の責務** (manual.md:14-26 から推測):
  - bench プロセスをキューイングして実行する
  - stdout JSON を parse してリーダーボードに反映する
- **運営手動の責務** (manual.md:287-294, manual.md:317-320):
  - **再起動による永続性検査** ("処理実施後に再起動が行われた場合、再起動前に行われた処理内容が再起動後に保存されている必要があります"): bench プロセスは再起動を伴わない 1 サイクル測定のみで、「再起動後にデータが残っているか」の検査は競技後の運営追試に委ねている
  - **ブラウザ表示確認** ("アプリケーションはブラウザ上での表示を初期状態と同様に保つ必要があります"): bench はブラウザレンダリングを検査せず、frontend 静的ファイル (js/css) の MD5 一致を verify で見るだけ
  - **パスワード平文保存の禁止**: bench からは検出できない。運営追試の責務
- **競技者の責務**:
  - DB 構造変更などを行った場合に `POST /initialize` で整合性を回復させる (manual.md:283-285)
  - 「ベンチマーカーとブラウザの挙動に差異がある場合、ベンチマーカーの挙動を正とする」(manual.md:198) — つまり frontend が壊れても bench が通れば競技上は OK と明示
- **bench はやらないこと** (= API として叩かれない、または機能として実装されない):
  - `POST /register` (新規会員登録) — bench は asset の既存 user pool でログインして検査するため、会員登録 API は load 中に呼ばれない (`webapp/go/main.go:357` で webapp は実装している)
  - frontend route (`r.Get("/")`, `/timeline`, `/categories/...` など、`webapp/go/main.go:360-372`) — bench は API のみ叩く
  - 商品画像の image upload bytes 一致 (`bench/scenario/verify.go:57-84` のように、verify scenario #1 で 1 商品ぶん md5 一致を見るのみ。他の load / check では item.ImageURL の path 一致と、verify 系の `verifyGetItem` で初期データに対してのみ md5 検査する `verify.go:809-824`)
  - 暗号化アルゴリズムや bcrypt cost のような「内部実装の質」の検査
  - DB スキーマの強制 (任意の DB 構造を許す、manual.md:194-197)

これらは「bench 責務外」として整理されるべきもので、後述 §3 / §6 では gap として扱わない。

## 3. 検査の構造

### 3.1 整合性検査 (verify phase)

`scenario/verify.go:15-366` の `Verify(ctx)` 関数。9 個の goroutine が並列で 1 回ずつ実行され、`wg.Wait()` で全完了を待つ。

各 scenario の概要:

- **#1 (`verify.go:18-91`)** — sell → 自分の出品ページから商品を見つける → 出品数が増えたか確認 → 商品画像 md5 一致 → buy 完了 (status 全フェーズ確認版)
- **#2 (`verify.go:93-121`)** — `/new_items.json` を 2 ページ巡回 → bump → bump 後に new_items / users.json で順序が更新されているか
- **#3 (`verify.go:123-152`)** — random root category の `/new_items/{id}.json` を 2 ページ巡回 → 出品済み商品の price を編集して 200 OK
- **#4 (`verify.go:154-219`)** — `/users/transactions.json` 巡回 → sell → 自他の users.json / new_items.json / transactions.json で見つかる → buy 完了
- **#5 (`verify.go:221-261`)** — buyer 自身の users.json 全件確認、active seller 自身の users.json 全件確認 (10 件以上ある前提)、非 active な buyer の users.json も 0 件可で見る
- **#6 (`verify.go:263-283`)** — random active seller 3 人の users.json を見て 5 商品の詳細確認
- **#7 (`verify.go:287-295`)** — 間違ったパスワードでのログイン拒否 (`irregularLoginWrongPassword`)
- **#8 (`verify.go:297-319`)** — irregular sell/buy フルコース (`scenario/wrong.go:30-195`、後述)
- **#9 (`verify.go:322-363`)** — `/static/` 配下の js/css ファイル MD5 一致検査 (asset.GetStaticFiles で列挙)

**画像 MD5 の引数源** (`verify.go:55-67`): bench が出品した商品については「出品時に bench が指定した画像ファイル」をローカルで md5 計算、initial-data の既存商品については `asset.GetImageMD5(aItem.ImageName)` で初期データに紐づく md5 を持つ (`verify.go:809-824`)。

#8 の `irregularSellAndBuy` (`scenario/wrong.go:30-195`) は特に厚く、「不正系を一通り 1 シーケンスで通す」黒板的シナリオ:

- 不正な CSRF トークンでの sell / buy / ship / ship_done が拒否されること
- 価格範囲外 (100 円未満 / 100 万円超) の出品が拒否されること
- 自分の商品は買えない (HTTP 403, "自分の商品は買えません")
- 残高不足カードでの決済が失敗する (HTTP 400, "カードの残高が足りません")
- on_sale でない商品は買えない / 編集できない
- QR コードは Ship 前 / 他人からは見えない
- accept 前の ship_done 拒否、他人からの ship/ship_done 拒否

各拒否について bench は **status code と error message 文字列の両方** を assert する (`session/wrongapp.go` 側、`bench/scenario/wrong.go:81,88,113` 等の `BuyWithFailed` 引数)。

### 3.2 負荷走行 (load)

`scenario/scenario.go:32-94` の `Validation(ctx, campaign)` が並列に以下を起動:

- `Check(ctx)` (常時 1 worker)
- `Load(ctx)` (worker 1 個目 + 100ms 後に worker 2 個目)
- campaign > 0 ならさらに `Load(ctx)` worker を `(i+2)*100ms` ずらして `campaign` 個追加 + `Campaign(ctx)` 1 個

**並列度の動的化** (`scenario.go:42-83`): キャンペーン還元率 (0〜4) によって Load worker 数が 2〜6 まで増えるという「設定ベースの負荷段階」設計。コメントに `"還元率の設定, 負荷, 人気者出品 / 0,2,なし / 1,3,あり / 2,4,あり ..."` と表が付く。

**Load の内部構造** (`scenario/load.go:25-416`): 4 種類のシナリオ (`#1`〜`#4`) を並列で回す。並列数は `NumLoadScenario1..4 = 1,2,2,1` (`load.go:19-23`)。各シナリオは 60s / 3s = 20 回ループする外周で、毎ループ `<-time.After(3 * time.Second)` でレートリミット (`load.go:57, 144` 等)。

各シナリオの設計意図 (cf. `load.go:29-35` のコメント):

> sell と buy の間に他の処理を挟む。今回の問題は決済総額がスコアになるので MySQL を守るために GET の速度を落とすチートが可能。それを防ぐために sell したあとに他のエンドポイントにリクエストを飛ばして完了してから buy される。

`load.go:32-35` のコメントから「最適化の偏りを許さないために、シナリオ 1 ループ内で多種エンドポイントを順番に踏ませる」という設計意図が読み取れる。

- **#1 (`load.go:42-150`)** — sell → recommend 判定 → recommend なら新着 10 ページ + 20 商品確認、recommend でなければカテゴリ × 7 を 10 ページずつ巡回 → buy without check → recommend なら 2 倍購入
- **#2 (`load.go:158-247`)** — sell → カテゴリ 30 ページ → transactions.json 10 ページ × 2 → buy without check
- **#3 (`load.go:254-340`)** — sell → active seller 3 人の users.json → 商品数の少ない user の users.json を 4 周 (`"indexつけるだけで速くなる"`) → buy with full status verify
- **#4 (`load.go:347-405`)** — sell → 新着 30 ページ 50 商品確認 → buy with full status verify

**`buyComplete` (without check)** vs **`buyCompleteWithVerify` (with check)** の使い分け (`scenario/action.go:108-296, 298-354`): 後者は「sell 後 / Ship 後 / ShipDone 後 / Complete 後」の 4 phase で「item / users.transactions の seller / buyer の合計 4 視点 × status / transaction_evidence_status / shipping_status の 3 軸」を全パターン assert する厚い検査。前者は最小限。

**worker pool** (`scenario/pool.go`): `ActiveSellerPool` / `BuyerPool` の 2 つの session キュー。dequeue → 使う → enqueue という「ログインずみ session の再利用」で web アプリ側の login 負荷を一定に保つ設計。

**fail fast 条件**: `Validation` 内では fail fast はせず、エラーを `fails.ErrorsForCheck.Add` で蓄積し続ける。critical 1 件 / application 10 件 / context.Done のいずれかで `Validation` から戻った後 `cmd/bench/main.go:158-173` で fail 判定する。

**Check の内部構造** (`scenario/check.go:14-263`): 4 つの `check scenario` を並列実行。

- **#1**: 間違ったパスワードでのログイン拒否を 8s 周期 (`check.go:24-43`、コメント: "これがないとパスワードチェックを外して常にログイン成功させるチートが可能になる")
- **#2**: random root category の new_items 巡回 + active seller 5 人 / 非アクティブ buyer 2 人の users.json + irregular sell/buy フルコース、10s 周期
- **#3**: bump → 反映を new_items / users.json で確認、5s 周期 ("bumpは投稿した直後だとできないので必ず新しいユーザーでやる")
- **#4**: sell → 編集 (price+10) → buy with verify、10s 周期

`check.go:24-43` の "パスワードチェックを外して常にログイン成功させるチートを防ぐ" のように **スコアハック対策のためのシナリオ** がコメント付きで明記されている。

### 3.3 最終チェック / 永続性検査の所在

bench 内に **final-check が存在する** (`scenario/scenario.go:96-143`)。内容:

1. payment mock の `GetReports` で「load 中に決済を通した itemID → price/status マップ」を取り出す (bench プロセス内のメモリから)
2. webapp の `GET /reports.json` を叩いて `transaction_evidences` を取得
3. webapp 側の各 transaction_evidence について payment mock 側に対応する記録があるか照合
4. price が一致しないなら application エラー
5. status が `done` のものだけ price をスコアに加算 (bench からの connection 切断による未確定状態を許容)
6. **payment mock 側に残っている = webapp 側に存在しない transaction_evidence は「購入されたはずなのに記録されていません」として application エラー** (`scenario/scenario.go:138-140`)

webapp の `getReports` (`webapp/go/main.go:2311-2322`) は `id > 15007` で初期データ分を除外するクエリ。「初期データを引き算して測定対象だけ返す」という portal/bench 連携用の API。

**永続性検査 (再起動後にデータが残っているか)** は bench 内には実装されていない。これは manual.md:289-290 で「処理実施後に再起動が行われた場合、再起動前に行われた処理内容が再起動後に保存されている必要があります」「予選参加日終了後、主催者からベンチマーク走行成績の追試、ならびにデータ永続化、画面表示に関するチェックが行われます」(manual.md:317-318) と明記されており、**運営手動 / 競技後の追試** に委譲する設計。bench は 1 サイクル走行を完結させることだけが責務。

## 4. 採用された手法 (再利用候補)

### 4.1 payment / shipment mock の bench プロセス内同居

**何を達成しているか**: 外部決済サービス (`POST /token`) と外部配送サービス (`POST /create`, `/request`, `/accept`, `/status`) が bench プロセス内 HTTP server として起動し (`cmd/bench/main.go:79-86`)、bench scenario 側から `ForceSet` (`payment.go:307-320`)、`ForceSetStatus` (`shipment.go:424-428`)、`GetReports` (`payment.go:329-334`) で内部状態を直接操作・観察できる。これにより:

- 「webapp が外部 API に渡したカード番号 / 金額」を bench が pre-register したものと突合できる (`bench/server/payment.go:230-241`)
- 「webapp が決済を済ませたが伝票上は未完了」のような状態を bench が能動的に作り出せる (campaign シナリオで「全員失敗」をエミュレート、`bench/scenario/wrong.go:88` の残高不足カード)
- 配送ステータス遷移を bench が任意のタイミングで進められる (`shipment.go:424-428` の `ForceSetStatus` で `StatusShipping` → `StatusDone` を強制)

**合同演習2026 で使えそうか**: 強く推奨。外部サービスを mock で bench 内に同居させる構造は、「webapp ⇔ 外部サービス」の境界の整合性を critical 化する手段として最も素直。Rust なら `axum` か `hyper` で同等構造が取れる。`ForceSet` 系の bench-only entrypoint を mock object のメソッドとして公開するパターンも素直。

### 4.2 「sell と buy の間に他エンドポイントを挟む」シナリオ設計

**何を達成しているか**: スコア源 (取引完了) 1 回あたりに「巡回・参照系の負荷」を必ず混ぜることで、特定エンドポイントだけ最適化する偏ったチートを防ぐ (`bench/scenario/load.go:29-35` のコメントに明記)。シナリオ 1 ループあたり sell × 1, buy × 1, GET 系 N + ship + ship_done + complete のセットを必ず通す。

**合同演習2026 で使えそうか**: 推奨。「スコア計算のソースになる action」と「整合性 / 並び順を検査する read action」をシナリオレベルで縛る手法は再利用しやすい。`time.After(3 * time.Second)` でループ周期を作る形 (`load.go:57`) はそのまま使える。

### 4.3 session pool による「ログイン済み session の再利用」

**何を達成しているか**: 各シナリオが session を `Dequeue` → 業務 → `Enqueue` で戻すことで、bench は十分なログイン回数を維持しつつもログイン自体は負荷の主役にしないようにできる (`bench/scenario/pool.go`、`bench/scenario/action.go:22-40`)。pool が空なら新規 login する fallback で「pool 不足にならない」ことを保証。

**合同演習2026 で使えそうか**: 推奨。Rust なら `Arc<Mutex<VecDeque<Session>>>` で簡単に書ける。「ログイン回数を負荷の主役にしない」という意図は、ログイン以外の機能を計測したい問題ではほぼ常に使える。

### 4.4 静的ファイル MD5 の「ディレクトリ参照式」検査

**何を達成しているか** (`bench/scenario/verify.go:322-363` および `cmd/bench/main.go:60` の `-static-dir`): bench にハードコードされた md5 リストを持たず、bench 起動時に指定された `static-dir` 配下のファイルを起動時にスキャンして md5 マップを生成、それと webapp が返す内容を比較する。コメント (`verify.go:323`) に "ベンチマーカーにmd5値を書いておく方針だと、静的ファイル更新時にベンチマーカーの更新も必要になるし、全く同じ静的ファイルを生成するのは数ヶ月後には困難になっている" と動機が記されている。

**合同演習2026 で使えそうか**: 採用候補。AMI に含まれる static asset を bench コマンド側で明示的にディレクトリ指定する formula。「md5 のハードコードを避ける」観点で、bench 配布のメンテナンス性を上げられる。

### 4.5 「人気者出品」(popular listing) の同時購入競合検査

**何を達成しているか** (`bench/scenario/campaign.go:124-281`): キャンペーン還元率が高いほど発火する追加シナリオ。1 商品に対して 80〜220 人が同時に購入を試み、`buyerCh chan *session.Session` のバッファ 1 に入れる。バッファ溢れ (= 2 人目以降が成功した) を検知すると critical エラー (`campaign.go:230-232`)。10% は意図的に決済失敗カードを使い、「全員成功してから 1 人勝ちロックを取る」自明な実装でもバレるようにしている (`campaign.go:156-167` のコメント "全員が成功するなら適当に1ユーザーでロックを取って、他のユーザーはエラーを返すだけで良い")。

**合同演習2026 で使えそうか**: 同時購入の排他性を検査する問題なら直接転用可能。`chan struct{}` バッファ 1 で「真の勝者は 1 人だけ」を判定する形は Rust の `tokio::sync::mpsc::channel(1)` で同等構造が取れる。

### 4.6 fail カテゴリ 3 段階 (`Critical` / `Application` / `Timeout`)

**何を達成しているか** (`bench/fails/fails.go:10-17`): エラーをカテゴリ分けして閾値を変える設計。critical は 1 件 fail、application は 10 件 fail、timeout は 200 件超で 100 件ごとに 5000 点減点 (`cmd/bench/main.go:160, 188, 204-207`)。`failure.MessageOf` が読めない予期しないエラーは critical に倒す保守的設計 (`fails/fails.go:91-94`)。

**合同演習2026 で使えそうか**: 採用候補。閾値の数値そのものは合同演習2026 のスコア感に合わせて調整するとして、「critical で即 fail / application は閾値方式 / timeout は寛容」の 3 段は実用的。Rust なら `enum FailKind { Critical, Application, Timeout }` を `Add` に渡す形に置き換えられる。

### 4.7 動的並列度: campaign 値で worker 数を変える

**何を達成しているか** (`bench/scenario/scenario.go:42-83`): `POST /initialize` のレスポンスで競技者が `campaign` (還元率 0〜4) を返してきた値そのものが、bench 側の Load worker 数の追加分になる。「アプリが速くなったら自分から負荷を引き上げよ」という宣言を競技者が行う設計。

**合同演習2026 で使えそうか**: 採用候補だが扱いは要検討。同様の自己宣言式は ISUCON で時々見られるパターン。Rust 単言語問題でも、競技者が `POST /initialize` で worker 規模を宣言し bench が忠実に従う形は組みやすい。

### 4.8 `req.Host` を target host に強制 + TLS SNI 制御

**何を達成しているか** (`bench/session/session.go:109,127,141` および `bench/session/new.go:18`): bench は実 IP に向けて HTTP リクエストを送りつつ、`req.Host` ヘッダと TLS の `ServerName` を `-target-host` (デフォルト `isucon9.catatsuy.org`) で固定する。これにより参照実装が name-based vhost / TLS 証明書を使うことができる (`manual.md:102-109` で `/etc/hosts` の追記指示)。

**合同演習2026 で使えそうか**: 強く推奨。AMI が nginx で TLS を終端し vhost 振り分けする形を取るなら、bench 側で SNI と Host を上書きする本パターンはほぼ必須。Rust なら `reqwest::ClientBuilder::resolve(host, addr)` 等で同等構造を作る。

## 5. 設計上の選択点 (横断タグ)

- `[慣習らしい: pretest/prepare = 整合性検査専念、load = 性能/scoring 専念の責務分離 (本回は initialize → verify → load → final-check の 4 段)]` (`cmd/bench/main.go:101, 120, 143, 177` のログ出力ラベルがそのまま phase 名として使われている)
- `[慣習らしい: critical / soft (warning) の二段階エラー分類 + soft 件数閾値で critical 化または fail fast (本回は critical 1 件 / application 閾値 / timeout 寛容 の 3 段 fail)]` (`fails/fails.go:10-17`)
- `[慣習らしい: 失敗系 (異常系) pretest を 4〜6 件入れて business invariant を critical 化する (本回は不正系を独立シナリオに集約 `irregular...`)]` (`scenario/wrong.go:30-195` をフル 1 シーケンスにまとめる)
- `[慣習らしい: load の並列度を成功カウンタや競技者操作で動的に引き上げる (本回は campaign / 還元率で worker 数を引き上げる「自己宣言式の負荷段階」)]` (`scenario/scenario.go:66-83`)
- `[慣習らしい: 外部サービス (payment / shipment) mock を bench プロセス内同居 + bench から内部状態を直接 inspect]` — bench プロセス内 HTTP server として起動し、bench scenario 側から内部状態を `ForceSet` / `GetReports` で直接 inspect する (`cmd/bench/main.go:79-86`、`bench/server/payment.go:307-334`、`bench/server/shipment.go:424-437`)
- `[この回特有: load 走行は 60s 固定、初期化 timeout 20s]` (`scenario/scenario.go:16, 21`、manual.md:240, 279)
- `[この回特有: load 中に random 順巡回・件数 / 並び順 assert]` (`load.go:483-510` 付近の created_at 単調減少 assert と `ItemsPerPage` 一致 assert)
- `[この回特有: 取引完了売上を score にする「業務金額型スコア」]` (`scenario/scenario.go:113-136`)
- `[この回特有: session pool でログイン回数を間引く]` (`scenario/pool.go`, `scenario/action.go:22-40`)
- `[この回特有: バッファ 1 channel で「真の購入勝者は 1 人」を判定する人気者出品検査]` (`scenario/campaign.go:133, 230-232`) — 同時購入競合のシンプルな表現
- `[この回特有: 静的ファイル md5 を bench 起動時にディレクトリスキャンして取る]` (`cmd/bench/main.go:60`、`scenario/verify.go:322-363`)
- `[この回特有: 外部サービスのレイテンシを「verify では入れず validation で 800ms 入れる」]` (`cmd/bench/main.go:147-148`) — verify を高速化しつつ load では現実的な遅延を再現
- `[この回特有: 「sell と buy の間に他エンドポイントを挟む」がシナリオ設計の表向きの題目になっている]` (`scenario/load.go:29-35` のコメント)
- `[この回特有: shipment mock が QR 画像 (PNG) を生成し、bench は画像 byte の md5 を webapp に通知して「QR が正しい画像か」を検査する]` (`bench/server/shipment.go:332-348`、`scenario/action.go:200-208`)
- `[この回特有: `/reports.json` を bench-only API として webapp に実装させる手法]` (`webapp/go/main.go:2311-2322` の `id > 15007` フィルタ) — final-check で payment mock の決済記録と突合するための bench 専用 endpoint
- `[この回特有: TLS SNI / Host header を `-target-host` フラグ 1 つで固定する設計]` — 13 / 14 の bench 内蔵 DNS とは対照的な、より素朴な targeting
- `[この回特有: load worker 内部の for ループを `time.After(3 * time.Second)` でレートリミットして「シナリオの最小再実行間隔」を作る]` (`load.go:57` 等)
- `[この回特有: payment mock を「決済額が違ったら critical エラーを記録しつつ処理は継続する」設計にしている]` (`payment.go:233`) — **推測:** 後続の状態整合性の崩れを bench に観察させ続けることで、不変条件の波及検査を意図しているように見える

## 6. 実装の不具合・残課題 (事実列挙のみ)

- **`bench/server/payment.go:249-251`**: `isValidOrigin` が `return true` を返す stub 実装。CORS Origin 検査を実質的に無効化している。コード上のコメント `// Originはちゃんとチェックしている前提のコード。コピペしないこと。` (`payment.go:258`) で意図的なものと明示されている。
- **`bench/server/payment.go:138-144`**: `reportStore.SetStatus` が `item := c.items[itemID]` の直接アクセス (`, ok` 形式での存在チェックなし) で zero-value をそのまま `item.Status = status` の起点にしており、未登録 itemID への `SetStatus` は zero-value entry を作って silently 成功する。`shipDone` (`scenario/action.go:362`) と `complete` (`scenario/action.go:372`) から呼ばれるが、`Set` を経ずに `SetStatus` だけが呼ばれるパスは `scenario/action.go` には現状ないため運用上の影響はない。
- **`bench/server/server.go:88-100`**: `userIP` の `True-Client-IP` ヘッダ処理が "未検証で信じる" コメント付きで残されている (`// 未検証で信じる` `// DO NOT COPY the following code`)。`// DO NOT COPY` コメントが付されており、ベンチ内 mock サーバの IP 制限のため、競技サーバ以外からの直接アクセスを防ぐ目的で意図的に保守的な実装と推測される。
- **`bench/scenario/verify.go:323-324`**: コメント "ベンチマーカーにmd5値を書いておく方針だと、静的ファイル更新時にベンチマーカーの更新も必要になるし、全く同じ静的ファイルを生成するのは数ヶ月後には困難になっている" — これは設計判断の記録であり不具合ではない (記載省略可)。
- **`bench/scenario/check.go:88` (check scenario #2 内)**: `for _, userID = range userIDs` のループ内で 1 件でも `checkUserItemsAndItems` が失敗すると `return` (`check.go:88`) してしまい、外側の `goto Final` 経由のレートリミット loop を抜けない代わりに goroutine そのものが終了する。他のシナリオは同種のエラーで `goto Final` するので一貫していない。
- **`bench/scenario/load.go:482, 571, 717`**: `MEMO 50件よりはみないだろう` / `MEMO 50ページ以上チェックすることはない` などのマジックナンバー前提のコメントが複数箇所に残っている。`loadIDsMaxloop = 100` (`scenario/normal.go:18`) との関係性に関する記述は無い。
- **`bench/scenario/verify.go:661`**: `if item.BuyerID == s.UserID && (item.SellerID == s.UserID && item.BuyerID != 0)` という条件式。前段の `item.BuyerID == s.UserID` と内側の `item.SellerID == s.UserID` を `&&` で繋いでおり、「buyer かつ seller が同一 user」のときだけ true になる。ただし isucari の業務上「自分の商品は買えない」(`scenario/wrong.go:81`) ため、この分岐が真になる経路は通常存在しない。`verify.go:886` にも同型の条件式がある。
- **`bench/scenario/normal.go:35`**: error メッセージ `"POST /initialize の還元率の設定値は %d以上 %d以下です"` が値範囲外時に返るが、`Messagef` の `%d` 引数が `MinCampaignRateSetting` と `MaxCampaignRateSetting` のみで「実際に来た値」を含めていない。デバッグの手がかりが薄くなっている。
- **`bench/scenario/scenario.go:16`**: `ExecutionSeconds = 60` がベタ書きで CLI / env から変更不可。`-exec-seconds` 等のフラグは存在しない。
- **`bench/scenario/load.go:616`**: `loadUserItemsAndItems` のコメント `// 多少のずれは許容` の `buffer := 10` がマジックナンバー。同種の `verify.go:704` では `buffer := 1` (`// verifyは厳しめ`) と差を付けているが、根拠は code 内には書かれていない。
- **`bench/scenario/action.go:413-419`**: `SetShipment` / `SetPayment` がパッケージ変数 `sShipment` / `sPayment` をグローバルに書く。`cmd/bench/main.go:84-85` の 1 回のみ呼ばれる前提だが、テスト等で再利用するときに状態がリークしうる構造。

## 7. この回固有の特殊事情

isucari は ISUCON 9 予選用に新規開発されたフリマアプリで、「外部サービス (決済 / 配送) との非同期な状態遷移」を業務不変条件の中心に据えた回。bench がプロセス内に payment / shipment mock を抱え込むのはこのドメインを擁護する手段として選ばれている (`§4.1`、`§5` の `[この回特有]` タグ群)。スコアが「決済総額」という business metric なので、final-check は「mock 側の決済記録 = webapp 側の transaction_evidences」の双方向 matching に集約される (`scenario/scenario.go:96-143`)。一方で永続性検査・ブラウザ表示確認は manual で明示的に「予選終了後の運営追試で確認する」(`manual.md:289-294, 317-318`) と切り分けられており、bench は 1 サイクル測定だけに責務を絞る。

competitive な「最近メンテされた古い回」の側面として `go.mod` が `go 1.23.0` を要求し (`go.mod:3`)、`for range int` (Go 1.22+) や `math/rand/v2` (Go 1.22+) を多用している (`scenario/load.go:42, 56` など)。元々の 2019 年実装からは大幅に modernize されているため、コードスタイルそのものは現代 Go のリファレンスとしても読める。
