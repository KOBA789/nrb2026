# ISUCON 12 本選 ベンチマーカー survey

> 反映先: `docs/authoring/norms.md` § 3

## 0. メタ情報

- 競技日: 2022-08-27 (本選)
- 題材: ISU CONQUEST (PC / スマートフォン向け放置ゲーム + ガチャ)
- 言語 (一次): Go
- bench code root: `../isucon/isucon12-final/benchmarker/` (flat 構成、CLI も同ディレクトリの `main.go`)
- webapp Go root: `../isucon/isucon12-final/webapp/go/` (主に `main.go` 2150 行 + `admin.go`)
- manual source:
  - 当日マニュアル: <https://gist.github.com/shirai-suguru/770d30d16688a07ba78e0a188cd99f9f>
  - アプリケーションマニュアル: <https://gist.github.com/shirai-suguru/accb96c5f86200b5c16e1d2a8b533cc1>
  - 講評: <https://isucon.net/archives/56959385.html>
- 補助参照:
  - `../isucon/isucon12-final/README.md`
  - `../isucon/isucon12-final/benchmarker/README.md` (本選後の修正点 = itemType=0 抜けの追加検査と x-isu-date を 8/27 固定にした旨)
  - portal-bench 連携は `main.go:14-15` で `isucon12-portal/bench-tool.go/benchrun` を import (= 12q と同じ橋)。

## 1. 全体構造

### 1.1 phase 構造

isucandar の Prepare → Load → Validation の 3 phase を取るが、`Scenario.Validation` は `return nil` の空実装 (`scenario.go:151-157`)。実質は 2 phase。

1. **Prepare** (`scenario.go:40-80`): タイムアウト `DefaultInitializeRequestTimeout = 1m` (`config.go:20`)。`LoadInitialData` で `dump/*.json` を読み (`prepare.go:12-34`)、`POST /initialize` を呼んで参照言語を取得 (`prepare.go:37-62`)、続けて `ValidationScenario` を 1 回まわす (`scenario.go:71`)。`ValidationScenario` 内の 12 段の検査列が**この回での「整合性検査」の本体**で、いずれか 1 段でも失敗すると Prepare ごと fail で打ち切る (`scenario_validation.go:856-1056`)。
2. **Load** (`scenario.go:89-149`): `LoadingDuration = 1m` (`config.go:31`)。4 種の worker を並列で回し、別途 `loadAdjustor` goroutine が 10 秒毎に並列度を増やす。
   - `NewLoginSuccessScenarioWorker` (= ログイン → home → reward → present → gacha 5 段、`scenario_login.go:17-81`)
   - `NewUserRegistrationScenarioWorker` (= POST /user → home → present → gacha → item → addexp → setdeck 7 段、`scenario_register.go:16-118`)
   - `NewBanUserLoginScenarioWorker` (= ban ユーザのログインが 403 で帰ることだけを確認、`scenario_banuser.go:17-43`)
   - `FireRefreshingMasterVersion` (= `MasterRefreshStartTime = 20s` 後に 1 回だけ master 更新を発火、`config.go:33`、`scenario_master_refresh.go:12-37`)
3. **Validation** (`scenario.go:151-157`): `if PrepareOnly { return nil }; return nil` のみ。bench 内 final-check は実装されていない (詳細 §3.3)。

### 1.2 scoring の概観

`score.go:30-43` の `ScoreRateTable` で 12 種の `ScoreTag` ごとに加点係数を定義 (login 3、create user 3、present-list 3、receive 2、gacha draw 2、home 1、その他 1)。減点はシナリオエラー 1 件あたり `ErrorDeduction = 15` (`score.go:46`)、合計が負なら 0 (`main.go:255-258`)。`/admin` 配下のリクエストは `ScoreTag` を持たないため加点対象外で、整合性チェックの中でしか叩かれない (= 当日マニュアル「/admin 配下へのリクエストは加点されません」と整合)。

エラー分類 (`apperror.go:15-36`):
- `initialize-error-*` / `validation-error-*` / `internal-error-*` のいずれかが 1 件でも積まれると `containsFatal()` (`main.go:71-73`) で fatal 判定 → スコア 0 + `passed=false` で portal に submit。
- `scenario-error-*` (= load 中の status code 不一致 / JSON decode 失敗 等) は減点だけで継続。`MaxErrors = 50` (`config.go:37`) を `loadAdjustor` で観測しつつ、`step.Result().Errors.Count()["load"]` がしきい値超で `step.Cancel()` し計測打ち切り (`scenario_helper.go:96-104`)。

### 1.3 CLI / env / 設定可能項目

CLI フラグ (`main.go:141-150`):

- `--target-host` (default `localhost:8080`、`config.go:15`)
- `--request-timeout` (default 3s、`config.go:18`)、`--initialize-request-timeout` (default 1m、`config.go:20`)
- `--exit-error-on-fail` (default true、`config.go:21`)
- `--stage` (default `test`、`config.go:22`) — `test` は 1 ループ + 並列度 1 固定、`prod` で `WithInfinityLoop` + `WithMaxParallelism(option.Parallelism)` (`scenario_helper.go:163-181`)
- `--max-parallelism` (default 100、`config.go:23`)
- `--prepare-only` (default false、`config.go:24`)

env: `ISUXBENCH_TARGET` (`benchrun.GetTargetAddress()`、`main.go:89-91`) と `ISUXBENCH_REPORT_FD` (`main.go:128`) で portal supervisor から起動される。`benchmarker/README.md:21-26` で `ISUXBENCH_TARGET` を上書き可能と明記。

### 1.4 入出力

portal にバイナリ protobuf で `BenchmarkResult{Finished:true, Score, ScoreBreakdown{Raw, Deduction}, Passed, Execution.Reason, SurveyResponse.Language}` を 1 回 report (`main.go:39-61` + `269-281`)。`Language` は `POST /initialize` のレスポンスから拾う (`prepare.go:53`)。

## 2. ドメインと擁護する不変条件

### 2.1 ベンチが「絶対に守らせる」business invariant

Prepare の `ValidationScenario` で 1 件でも失敗すると即 fail (`apperror.go:71-73` + `main.go:104-122`)。Prepare 段で次が落ちると bench 全体が 0 点で終わる:

- **ログイン応答の構造的整合**: `POST /login` 成功時に sessionID / viewerID / updatedResources.user.* / login bonus 反映が期待通り。失敗ケース (存在しない userID) は status 404 + `{"statusCode":404,"message":"not found user"}` (`config.go:51-52`、`validation_user.go:172-195`)。
- **ログインボーナスの日跨ぎリセット**: `time.Date(2022, 8, 26, 14, 59, 59, ...GMT)` (= JST 8/26 23:59:59) と `time.Date(2022, 8, 26, 15, 0, 1, ...GMT)` (= JST 8/27 00:00:01) の 2 回ログインで `userLoginBonuses[0].LastRewardSequence` が `+1` 進む / `LastLoginBonusSequence (28)` を超えたら 1 に戻る挙動を確認 (`scenario_validation.go:903-913`、`config.go:50` + `validation_user.go:219-220`)。
- **ユーザ作成時の付与アイテム**: `POST /user` のレスポンスで `userCards` が 3 枚 / 各 `CardID=2, AmountPerSec=1, Level=1` / `userDecks` が 1 件 / `userLoginBonuses` が初期 sequence=1 で揃っているか (`validation_user.go:62-131`) と、現在時刻に該当する `presentAllMasters` 行が `userPresents` に展開されているか (`validation_user.go:132-166`)。
- **マスターバージョン無効時の 422**: `x-master-version` が現行 active と異なる場合 `StatusUnprocessableEntity` (`webapp/go/main.go:163-164`)。Load 段ではこれを契機に worker が `Rewind` する (詳細 §4)。
- **ban ユーザの 403**: ban 済みユーザがログインを試みると `LoginBanStatusCode = 403` + `LoginBanMessage = "forbidden"` を返す (`config.go:53-54`、`scenario_banuser.go:67-72`、`validation_user.go` 系列)。
- **他人セッションでの home 取得拒否**: 別ユーザの session で `GET /user/{userId}/home` を叩くと拒否される (`scenario_validation.go:344-379`)。
- **ワンタイムトークンの二重消費拒否**: `POST /user/.../card/addexp/{cardID}` と `POST /user/.../gacha/draw/.../10` は同一 token で 2 回目を叩くと失敗レスポンスを返す (`scenario_validation.go:580-604, 828-850`)。これらは PostUser → ItemList で受け取った `OneTimeToken` を**1 回限り**でしか使えないことを bench が積極的に検査している。
- **マスター更新ジョブの単一成功**: `FireRefreshingMasterVersion` で 1 回だけ発火する master 更新が失敗すると、特殊エラー `ErrCannotRefreshMasterVersion = "internal-error-cannot-refresh-master"` (`apperror.go:35`) で fatal 化 (`scenario_helper.go:13-37` + 「マスター更新に失敗した場合、失格となります」コンテスタント表示)。

### 2.2 責務分担の境界線

| 責務 | 主体 | 根拠 |
|---|---|---|
| 初期化リクエスト送信 / 整合性検査 12 段 / 1 分間の負荷走行 / master 更新 1 回 | bench | `scenario.go` 全体 |
| 整合性検査の 12 段が 1 回でも落ちたら fail で 0 点 | bench (Prepare 段で発火) | `apperror.go:71-73`, `main.go:104-122` |
| 再起動後の永続性検査 (= load 走行で書いたデータがサーバ再起動後に取れるか) | portal の追試 + 運営の目視 | 当日マニュアル「負荷走行実行時にアプリケーションに書き込まれたデータが、サーバー再起動後にも取得できることが確認できなかった場合も失格」(bench 内には対応 phase なし) |
| `sudo systemctl reboot` 実行可能性、再起動後の動作確認、3 回追試の合否判定 | 運営 + portal | 当日マニュアル「再起動テスト」節 |
| ブラウザでアプリが正常動作するかの確認 (Unity build / フロント表示) | 運営の目視 | 当日マニュアル「ブラウザ確認時にアプリケーションが正常動作しない」が失格条件 |
| `/admin` 配下のリクエスト | bench は整合性検査の中でだけ叩く (= 加点対象外、`score.go` の `ScoreTag` に admin 系が無い) | 当日マニュアル「/admin 配下へのリクエストは加点されません」「/admin 配下へのリクエストは整合性チェックのみ行います」と整合 |
| `DELETE /admin/logout` の 204 確認 | bench (master 更新シナリオ末尾で 1 回叩く、`scenario_master_refresh.go:79-82`、`scenario_helper.go:154-178`) | bench 自身が 1 回だけ呼ぶが、ユーザ向けの `/login` には対応する logout 経路を bench は叩かない (= 競技者が削除可能な経路) |
| Validation phase (`Scenario.Validation`) | 何もしない (`scenario.go:151-157` で `return nil`) | bench 内 final-check 相当が無いことについては §3.3 |

### 2.3 ドメインモデル概観

bench が把握するロールは以下 (`model.go`、`scenario.go:13-36`):

- `User` (= 通常プレイヤー、3 種に分かれる: `royalUser` / `combackUser` / `oneYearUser` の `dump/*.json` で初期投入、`prepare.go:12-23`)
- `BanUser` (= ban 済みプレイヤー、`dump/banUserInitialize.json`、`prepare.go:25-28`)
- `Platform` (= 新規ユーザ登録時の platform identity、`dump/platforms.json`、`prepare.go:29-32`)
- `ValidationUser` (= 整合性検査の事前計算結果込みデータ、`dump/validateUserInitialize.json`、`scenario_validation.go:864`)
- `AdminUser` (= 管理者、ID 固定 123456 / pass `password`、`scenario.go:59-62` + `config.go:55-56`)

## 3. 検査の構造

### 3.1 整合性検査 (Prepare 内 ValidationScenario)

発火: `Scenario.Prepare` → `Scenario.ValidationScenario` (`scenario.go:71`、本体は `scenario_validation.go:856-1056`)。事前に `dump/validateUserInitialize.json` + 各種マスター JSON (`expItemMaster`, `cardMaster`, `loginBonusRewardMaster`, `gachaAllItemMaster`, `presentAllMaster`) を読み込む (`scenario_validation.go:864-894`)。

順序立てて実行される 12 段 (`scenario_validation.go:902-1053`):

| # | リクエスト | 検証関数 | 主眼 |
|---|---|---|---|
| 0.0 | `POST /login` (8/26 23:59:59 GMT 換算) | `loginValidateBeforDaySuccessScenario` (`scenario_validation.go:145-179`) | 日跨ぎ「前」のログインボーナス sequence |
| 0.1 | `POST /login` (8/27 00:00:01 GMT 換算) | `loginValidateAfterDaySuccessScenario` (`scenario_validation.go:109-143`) | 日跨ぎ「後」の sequence +1 反映 |
| admin.1 | `POST /admin/login` (誤 ID + 誤 PW、誤 ID 単独) | `postAdminLoginFailValidateScenario` (`scenario_admin_validation.go:11-72`) | 401 / 期待 message |
| admin.2 | `POST /admin/login` 成功 | `postAdminLoginValidateSuccessScenario` (`scenario_admin_validation.go:74-105`) | session id 取得 |
| admin.3 | `GET /admin/user/:userID` | `getAdminUserValidateSuccessScenario` (`scenario_admin_validation.go:167-195`) | 管理者から見たユーザ詳細 |
| L1 | `POST /login` 失敗 (存在しない userID) | `loginValidateFailScenario` (`scenario_validation.go:31-68`) | 404 + `not found user` |
| L2 | `POST /user` (新規作成) | `postUserValidateSuccessScenario` (`scenario_validation.go:70-107`) | initial card×3 / deck×1 / login bonus / present 全件 |
| L3 | `POST /login` 成功 | `loginValidateSuccessScenario` (`scenario_validation.go:181-222`) | session 払い出し |
| L4.0 | `GET /user/:id/home` (壊れ session) | `ShowHomeValidateFailScenario` (`scenario_validation.go:261-300`) | 401 |
| L4.1 | `GET /user/:id/home` 成功 | `ShowHomeValidateSuccessScenario` (`scenario_validation.go:303-341`) | login bonus item |
| L4.2 | `GET /user/:id/home` 他人 session | `ShowOhterHomeValidateFailScenario` (`scenario_validation.go:344-379`) | 403 |
| L5 | `POST /user/:id/reward` | `postRewardValidateSuccessScenario` (`scenario_validation.go:382-418`) | 放置中の isu coin の払い戻し計算 |
| L6 | `POST /user/:id/card` (deck 入替) → `GET /home` で反映確認 | `postCardValidateSuccessScenario` (`scenario_validation.go:463-526`) | deck 永続化 |
| L7 | `GET /user/:id/item` | `ShowItemValidateSuccessScenario` (`scenario_validation.go:421-460`) | item 一覧 + `oneTimeToken` 払い出し |
| L8 | `POST /user/:id/card/addexp/:cardID` (本番 + 同 token で再投) | `postAddExpCardIDValidateSuccessScenario` (`scenario_validation.go:529-605`) | exp 加算 + **double-submit 拒否** |
| L9 | `GET /user/:id/present/index/1` | `AcceptGiftValidateSeccessScenario` (`scenario_validation.go:607-649`) | present 一覧 |
| L10 | `POST /user/:id/present/receive` → `GET /present/index/1` (空) → `GET /item` (反映) | `postReceivePresentValidateSeccessScenario` (`scenario_validation.go:651-732`) | 二重消費なし + item 反映 |
| L11 | `GET /user/:id/gacha/index` | `GetGachaListValidationSuccessScenario` (`scenario_validation.go:735-776`) | gacha 一覧 + token |
| L12 | `POST /user/:id/gacha/draw/:gachaID/10` (本番 + 同 token で再投) | `postGachaDrawValidateSuccessScenario` (`scenario_validation.go:779-851`) | gacha 結果 + **double-submit 拒否** |
| admin.4 | `GET /admin/master` | `getAdminMasterValidateSuccessScenario` (`scenario_admin_validation.go:137-165`) | gacha + login bonus master の整合 |
| admin.5 | `POST /admin/user/:userID/ban` | `postAdminUserBanValidateSuccessScenario` (`scenario_admin_validation.go:107-135`) | ban 反映 |
| L13 | `POST /login`(=直前で ban されたユーザ) | `loginBanValidateScenario` (`scenario_validation.go:224-259`) | 403 |

レスポンス整合は `Diff()` (`validation.go:195-228`) による reflect ベースの構造体フィールド一致 (`json` tag を name とする)。エラーは `IsuAssert` / `IsuAssertStatus` で `ValidationErrInvalidResponseBody` / `ValidationErrInvalidStatusCode` を起票 (`validation.go:168-188`)。Prepare 段で 1 件でも積まれると fatal。

特徴的なパターンとして、整合性検査内で「成功 → 同じパラメータで再投すると失敗」の double-submit 検査を 2 種類 (addexp, gacha draw) で組んでいる (`scenario_validation.go:580-604, 828-850`)。これは「ベンチが擁護する business invariant: ワンタイムトークンによる重複防止」を能動的に検査している箇所で、§4 で再利用候補として扱う。

### 3.2 負荷走行 (load)

`Scenario.Load` (`scenario.go:89-149`) が 4 worker + 1 loadAdjustor goroutine を並列で回す。`LoadingDuration = 1m` の `isucandar.WithLoadTimeout` (`main.go:160`) で全体 timeout。

**worker 並列度の動的調整** (`scenario_helper.go:84-150`):

10 秒毎に `step.Result().Errors.Count()["load"]` を観測:
- `total >= MaxErrors (50)` で `step.Result().Score.Close(); step.Cancel(); return` → 計測打ち切り (`scenario_helper.go:97-104`)。これが**唯一の bench 側 fail-fast 経路**で、portal 側に load 段で fatal を投げるルートは別経路 (master 更新失敗 = `internal-error-*`、または status 不一致以外の構造的破壊 = `validation-error-*`)。
- `diff > 5` (= 直近 10 秒で 5 件以上エラー増) なら並列度を上げない、それ以外なら login 成功数 / user 登録成功数を見て login worker と user 登録 worker の並列数を `1, 3, 6, 9, 12, 15` / `1, 2, 4, 8, 16` の階段で増やす (`scenario_helper.go:115-141`)。
- ban worker は `(loginParallels + userRegistrationParallels) / 10` で連動 (`scenario_helper.go:146`)。

**シナリオ内での Rewind ループ** (`scenario_login.go:35-77` 等):
HTTP ステータス `422 (StatusUnprocessableEntity)` が返ると `Rewind()` を返し、worker は `goto Rewind` でシナリオを最初からやり直す (`scenario_login.go:113`, `157`, `194`, `240`, `283`, `329`, `366`)。422 は `apiMiddleware` (`webapp/go/main.go:163-164`) が `x-master-version` mismatch を検出した時の応答で、master 更新を踏んだ worker がそのまま fail せずシナリオ先頭 (= 最新の `LatestMasterVersion()` を再取得) からやり直す設計 (詳細 §4)。

**バックオフ**: `SleepWithCtx(ctx, time.Millisecond*500)` がある程度入る (`scenario_banuser.go:35`) が、login / user 登録 worker は意図的に sleep を入れず連続 hit。ロード制御は loadAdjustor の並列度調整で行う。

**Load 段でのレスポンス検査の浅さ**: load 段の 4 worker は status code (200 か 422 か) と `WithJsonBody[T]` (= JSON として decode できるか) の 2 点しか見ていない (例 `scenario_login.go:118-124, 161-164, 200-205`)。Body の意味的整合 (= 残高がいくらか / カードの種類が何か) は Prepare 段の整合性検査で 1 度だけまとめて検査する設計。

`scenario_master_refresh.go:12-37` の `FireRefreshingMasterVersion`: load 開始 20 秒後に 1 回だけ `RefreshMasterDataScenario` を発火 (`config.go:33`)。中身は admin login → `PUT /admin/master` (`scenario_helper.go:122-151`、multipart で `version_master.csv` + `present_all_master.csv` を投入) → `DELETE /admin/logout` の 3 段で、いずれかが落ちると `ErrCannotRefreshMasterVersion` (`apperror.go:35`) で internal error → fatal。`PUT /admin/master` のレスポンスから新しい `MasterVersion` を取り出し (`scenario_helper.go:144`)、scenario 全体で `s.UpdateMasterVersion()` (`scenario_helper.go:48-53`) して以降の worker からは `LatestMasterVersion()` 経由で見える。

### 3.3 最終チェック / 永続性検査の所在

bench 内 `Scenario.Validation` (`scenario.go:151-157`) は `if PrepareOnly { return nil }; return nil` のみで、isucandar の 3 phase のうち validation 段が**実装上空**である。bench 内 final-check 相当の処理は無い。

永続性検査については当日マニュアル「負荷走行実行時にアプリケーションに書き込まれたデータが、サーバー再起動後にも取得できることが確認できなかった場合も失格」とあり、portal の競技終了後追試 (各チーム 3 回再起動後計測、fail なら 4 回目) が担う設計 (= §2.2 に整理した責務分担)。bench 1 回の走行で見えるのは「Prepare の整合性検査 → Load 走行 → 即終了」までで、再起動後に再走行された時に同じ Prepare がクリアできれば永続性 OK と判定される。

`PUT /admin/master` の成功は当日マニュアル「`PUT /admin/master` のエンドポイントが成功していなかった場合は追試でも目視確認されます」と整合し、bench 内では master 更新ジョブの fail を `internal-error-*` に上げて fatal 化することで「load 1 回内」の失敗だけ拾う。

## 4. 採用された手法 (再利用候補)

### 4.1 `x-isu-date` ヘッダによる時刻支配

`setIsuDate` (`action.go:388-399`) が**全リクエスト**に `x-isu-date: <RFC1123>` ヘッダを付与し、webapp 側は `apiMiddleware` (`webapp/go/main.go:147-151`) でこれを `requestTime` として `time.Parse(time.RFC1123, ...)` し、context に詰める (`getRequestTime` で全 handler が参照)。

達成していること: ログインボーナスの「日跨ぎリセット」を bench から決定論的に検査できる。`scenario_validation.go:158` の `time.Date(2022, 8, 26, 14, 59, 59, 0, myGMT)` (= JST 8/26 23:59:59) と `scenario_validation.go:122` の `time.Date(2022, 8, 26, 15, 0, 01, 0, myGMT)` (= JST 8/27 00:00:01) の 2 回連続ログインで「sequence が +1 すること」を 1 秒差で再現可能になる。さらに `setIsuDate` は `masterVersion >= 2` のとき `xIsuDate += 24h` (`action.go:393-397`) を自動で挟むので、**master 更新後の load 走行は仮想的に「翌日」として動き、各ユーザのログインボーナスがもう 1 段進む**。これにより Prepare の検査値と Load 走行で見るユーザ状態の食い違いを意図的に作り、「日跨ぎが正しく動いているか」を Load 走行中も間接的に圧する。

合同演習2026 で使えそうか: 時刻に依存する処理 (= バッチ集計、TTL 切れ、cron) を扱うなら極めて有効。webapp 側に「ヘッダがあればそれを time.Now() の代わりに使う」契約を 1 行入れるだけで、bench からの決定論的シナリオ駆動が可能になる。

### 4.2 `x-master-version` 422 + worker の `Rewind`

`setMasterVersion` (`action.go:376-378`) が全リクエストに現行 master version を付け、webapp は `apiMiddleware` (`webapp/go/main.go:163-164`) で active master と一致しなければ `StatusUnprocessableEntity (422)` を返す。bench worker は 422 を見ると個別シナリオを **失敗扱いせず** `Rewind()` を返し (`scenario_login.go:112-114, 157-159, 194-196, 240-242, 283-285, 329-331, 366-368`)、`goto Rewind` でシナリオ先頭から `LatestMasterVersion()` を読み直してやり直す (`scenario_login.go:35-38`)。

達成していること: master 更新ジョブが load 中の任意のタイミングで走っても、worker は「古い version で叩いていた途中の操作」を捨てて新 version でやり直すだけで済む。減点や失敗にならない。`UpdateMasterVersion` (`scenario_helper.go:48-53`) が排他をかけて scenario 全体で 1 つの `MasterVersion` を共有する仕組みも素直 (RWMutex)。

合同演習2026 で使えそうか: スキーマ進化 / マスター入替の系を扱うなら直接転用可能。「進行中のリクエストを fail させずに巻き戻す」モデルは、bench が「業務トランザクション」として組まれているシナリオ (= 複数 step の連鎖) と相性が良い。Rust で書く場合は `goto` の代わりに `loop { ... continue; }` か `'rewind: loop { ... continue 'rewind; }` で書ける。

### 4.3 Prepare に整合性検査を全部寄せ、Load は status + 復号性しか見ない

Load の 4 worker は `WithStatusCode(200)` + `WithJsonBody[T]` (= 「Content-Type が application/json で、struct に decode できる」) の 2 点しか検査しない (`scenario_login.go:118-124, 200-205`、`validation.go:90-105, 136-140`)。意味的な field 値の検査は **すべて Prepare の整合性検査 12 段に寄せている** (`scenario_validation.go` の各 `validate*` 関数群)。

達成していること: Load 段はスループット最大化を最優先にでき、エラー判定の閾値も「N 件以上で打ち切り」というシンプル設計が可能。整合性が崩れていれば必ず Prepare で先に検出されるため、Load 中の隠れた regression がスコアに化ける危険を Prepare で先に閉じている。

合同演習2026 で使えそうか: 12q や 13/14 と比較しても整合性検査の責務分割 (Prepare = 精度、Load = 性能) が明快。bench を新規設計する際の baseline として有用。デメリットは「Load 中の意味的整合不全 (= 競技者が壊しても Prepare をたまたま通り抜ける regression)」を取りこぼす可能性で、これは double-submit 検査などの「同 token で 2 回叩く」系を整合性検査側に組み込むことで一部カバーしている (§4.4)。

### 4.4 同 token で 2 回叩く double-submit 検査

`postAddExpCardIDValidateSuccessScenario` (`scenario_validation.go:580-604`) と `postGachaDrawValidateSuccessScenario` (`scenario_validation.go:828-850`) は、成功した直後に「全く同じ `OneTimeToken` で同じパラメータの POST」を 1 回追加し、`validateFailResponse` で**期待されるエラーステータスが返ってくること**を検査する。webapp 側 `checkOneTimeToken` (`webapp/go/main.go:233-...`) の token 消費が機能しているかを能動的に検査する形。

達成していること: アイテム増殖 / ガチャ無限引きという「ゲーム業務上致命的な」business invariant を 1 つの sequential scenario で検査できる。アプリケーションマニュアルの「同じリクエストが 2 回実行されるケースを想定して、アイテムが増殖しないように制御」と整合する形での検証。

合同演習2026 で使えそうか: 在庫 / 残高 / 1 回限りクーポン系のドメインなら有効。bench 自身が攻撃者っぽい挙動を「正常 → 即追撃」のパターンで一連にすることで、整合性検査が business invariant の検査を兼ねるようになる。

### 4.5 worker pool 並列度を「成功数の階段」で動的に増やす

`loadAdjustor` (`scenario_helper.go:84-150`) は固定並列度ではなく、login 成功数 / user 登録成功数の累計から「次の 10 秒の並列度」を `1, 3, 6, 9, 12, 15` / `1, 2, 4, 8, 16` の階段で更新する。さらに直近 10 秒のエラー増分が 5 件超なら並列度を上げない (= ramp-up を一時停止) というガード。

達成していること: 競技者の最適化が進むほど自然に並列度が上がり、捌けないと自動でブレーキがかかる。固定並列度より「秒間スコア」の上限を作りにくい。

合同演習2026 で使えそうか: 単一 client 処理の最適化を測定するなら過剰設計だが、競技として「捌ける限り重みを上げる」必要があるなら有効。指標として「login 成功数」「user 登録成功数」のように bench 自身が「どのシナリオを進めたいか」を選んで反映できる点が良い。

### 4.6 `dump/*.json` での事前計算済みユーザの prefill

bench は `royalUser` / `combackUser` / `oneYearUser` の 3 ペルソナをそれぞれ事前生成して dump (`prepare.go:12-23`)、整合性検査用に「次にログインしたら login bonus seq がいくつ進むか / 何の present が来るか」まで計算済みの `validateUserInitialize.json` を別に持つ (`scenario_validation.go:864`)。

達成していること: 整合性検査での「期待値」を bench 側で **静的に** 持てる。webapp 実装と独立に「答え」を握っている形で、競技者が webapp を最適化しても期待値の正解は揺らがない。dev 用に 3 種のユーザ属性を均等分布で投入すると言うコメント (`scenario_login.go:22-23`) も、ランダムインデックスでも分布を維持する目的。

合同演習2026 で使えそうか: ユーザ属性に応じた挙動差を持つドメインで有効。生成スクリプト (`dev/extra/initial-data` 想定) で「webapp に投入する初期データ」と「bench が握る期待値」を同時に生成しておく構成が前提。

## 5. 設計上の選択点 (横断 synthesis 用タグ)

- `[慣習らしい: pretest/prepare = 整合性検査専念、load = 性能/scoring 専念の責務分離]` (`scenario.go:71`、`scenario_login.go:118-124` 等)
- `[慣習らしい: 永続性検査は portal の再起動追試に委譲 (`Scenario.Validation` は `return nil` の空実装)]` (`scenario.go:151-157`)
- `[慣習らしい: portal 連携は protobuf BenchmarkResult を `ISUXBENCH_REPORT_FD` に書き戻す (`benchrun.NewReporter`)]` (`main.go:14-15, 89-91, 269-281`)
- `[慣習らしい: scoring は ScoreTag × 倍率の線形加点 + エラー件数 × 固定減点]` (`score.go:30-46`)
- `[慣習らしい: load 打ち切りは「エラー件数 ≥ MaxErrors」の単一閾値で fail fast]` (`scenario_helper.go:96-104`、`config.go:37`)
- `[慣習らしい: 初期データ + 期待値を bench に同梱 (JSON dump)]` (`prepare.go:12-34`、`scenario_validation.go:864-894`)
- `[慣習らしい: load の並列度を成功カウンタで動的に引き上げる]` (`scenario_helper.go:84-150`)
- `[慣習らしい: shared-state を `sync.RWMutex` で守る `LatestMasterVersion()`]` (`scenario_helper.go:40-53`)
- `[この回特有: `x-isu-date` ヘッダで bench から webapp の time を支配し、ログインボーナス日跨ぎを 1 秒差で検査可能にする]` (`action.go:388-399`、`webapp/go/main.go:147-151`)
- `[この回特有: `setIsuDate` 内で `masterVersion >= 2` なら `+24h` を自動加算し、master 更新後の load 走行を「翌日」として動かす]` (`action.go:393-397`)
- `[この回特有: `x-master-version` 422 で worker が `Rewind()` してシナリオ先頭からやり直す]` (`scenario_login.go:112-114, 35-38`、`webapp/go/main.go:163-164`)
- `[この回特有: 整合性検査内に「成功 → 同 token 即追撃」の double-submit 検査を 2 種組み込む]` (`scenario_validation.go:580-604, 828-850`)
- `[この回特有: master 更新ジョブを load 開始 20 秒後に 1 回だけ発火]` (`config.go:33`、`scenario_master_refresh.go:12-37`)
- `[この回特有: master 更新リソースが見つからない (`./resource/version_master.csv` 等が無い) 場合は `os.Exit(1)` で即落とし、運営 Slack 通知前提とする]` (`action.go:272-285`)
- `[この回特有: ban worker の並列度が `(login + register) / 10` という連動式]` (`scenario_helper.go:146`) — login + register が 0 のとき AddParallelism(0) になるが許容

## 6. 実装の不具合・残課題 (事実列挙のみ)

- `apperror.go:86` `// FIXME ちゃんと isucandar の実装を読んで実装を直す。` — `IsIsucandarMarkedError` が `prepare`/`load`/`validation` プレフィックスで isucandar 内部由来エラーを「分類できないもの」として握り潰す実装が暫定であることを示す FIXME。
- `scenario_login.go:26-33` `s.ConsumedUserIDs.Add(int64(trial))` で**インデックス値**を Add しているが、`scenario_login.go:33` の `defer s.ConsumedUserIDs.Remove(user.ID)` で**ユーザ ID** を Remove している。Add と Remove のキーが食い違っているため、`ConsumedUserIDs` から trial 値が永続的に取り除かれず、長く回すと `for { trial := rand.Intn(...); if !Exists(...) ... }` の検索が遅くなる方向の bug。同セット型 `LightSet` は `map[int64]empty` の単純な map (`data/set.go:103-142`) なので削除されないだけで panic はしない。
- `scenario_validation.go:805` `num := rand.Intn(len(gachaData) - 1)` — `len(gachaData)` が 0 なら `rand.Intn(-1)` で panic、1 なら `rand.Intn(0)` で panic する。整合性検査内なので Prepare 段で fatal 落ちする経路。`gachaAllItemMaster.json` の内容と `xIsuDate (= 8/27)` の組み合わせで該当 gacha が 2 件以上残るデータ前提で動いている。
- `scenario_login.go:353` `gachaTypeID := 37` がハードコード。Load 段の gacha draw シナリオが常に同じ gacha id を叩く形 (整合性検査側はマスターから動的に選ぶ `scenario_validation.go:805-807`)。
- `scenario_login.go:103-105` および同型コード多数: `now := time.Now()` を取った直後に `xIsuDate := time.Date(2022, 8, 27, now.Hour(), now.Minute(), now.Second(), now.Nanosecond(), myGMT)` で日付だけ 8/27 GMT に固定する dead variable パターン。`now` は `xIsuDate` の構築以外に使われない (= 一旦 `time.Now()` を取得する必要が無い)。`benchmarker/README.md:50` 「x-isu-date を 8/27 になるように修正しました」(本選後の修正点として明記) との関係はコードからは断定できない。コードとしての副作用や bug は無い。
- `validation.go:168-188` `IsuAssert[T comparable]` および `IsuAssertStatus`、`Hint(endpoint, what string)` の `what` 第 2 引数を多くの呼び出し側で `""` で渡している (例 `validation_user.go:18, 176`)。`makeInconsistentMsg` がそのまま埋め込むので「`POST /user の Body の  が違います`」のように二重スペースになるが、メッセージ生成上の cosmetic で挙動には影響なし。

## 7. この回固有の特殊事情

放置ゲーム + ガチャという題材ゆえ、「時刻が業務ロジックの中心」「マスター更新を競技中に挟む」「ワンタイムトークンによる重複防止」「ログインボーナスの sequence/loop」「複数ペルソナ (royal / comback / oneYear / ban) の挙動分岐」が互いに絡み合うドメインになっている。bench はこれらを (a) 時刻支配 = `x-isu-date`、(b) master 入替 = `x-master-version` + `Rewind`、(c) 重複防止 = double-submit 検査、(d) ペルソナ分岐 = `dump/*.json` の事前計算済み期待値、という 4 つの仕組みで Prepare に集約して検査している。`Scenario.Validation` を空にして bench 1 回の走行内で「最終整合性」を計らない代わりに、portal の再起動追試 (`bench` を再走行 → Prepare の整合性検査が再度通る) で永続性を担保する設計判断。
