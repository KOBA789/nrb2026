# ISUNARABE 合同演習2026 作問記 — AI agent 主導で作問する 5 日間

このリポジトリは ISUNARABE 合同演習2026 の作問物 (webapp + benchmarker + AMI/CFn/bench.sh
+ problem.json) を収めたものである。5 日間の作業記録をここに残す。

作問は 2026-05-05 〜 2026-05-09 (本番当日) の 5 日間で、ほぼすべての作業を AI agent 経由
で進めた。本記事の引用は当時の Claude Code セッションログ (`~/.claude/projects/`) と
git の origin ログから採った。

## 道具立て

- **Claude Code** (Opus 4.7, 1M context) — 主ドライバ。作問期間中に 53 個の主セッション +
  10 個の worktree セッションを記録している。
- **Codex CLI** (OpenAI) — `smart-friend` という skill 経由で呼び出す「セカンドオピニオン」
  係。CLAUDE.md にレビューを必須としている (後述)。
- **superpowers** plugin (claude-plugins-official) — `brainstorming` / `writing-plans` /
  `subagent-driven-development` / `using-git-worktrees` / `requesting-code-review` 等の
  skill バンドル。docs ブートストラップの 2 日間 (5/5〜5/6) は brainstorming → spec → plan
  → subagent fan-out の重いフローで使ったが、5/7 朝に上流工程を降ろし、軽い相談 +
  smart-friend レビューの loop に切り替えた (詳細は本文)。実装フェーズには馴染まず、
  事実上 docs フェーズ専用の道具になった。
- **git worktree** — 実装フェーズの並列作業 (SPA 新規作成、AMI bring up、配布 strip 機構
  など 9 本) で素の `git worktree add` を使った。superpowers の `using-git-worktrees` skill
  自体は途中で使わなくなった (詳細は本文)。
- **subagent fan-out** — 同一セッション内で複数の subagent を並列に走らせる手法。歴代
  ISUCON 5 セットのベンチマーカー survey を 5 並列で書かせるなどに使った。

なお自動 memory は明示的に無効化した (`memory/MEMORY.md`: 「このファイルは使わない。
リポジトリ内の docs/ 以下を使うこと」)。session を跨いで持ち越したい知識は全部 docs に
書き、毎セッション冒頭で `docs/README.md` から読み込ませる運用とした。CLAUDE.md
冒頭の "Reading order" がそれを強制している。

## CLAUDE.md に書いた中核ルール

3 つの設計指針 (webapp / bench / その他) と、ワークフロー上の強い 2 つのルールを
最初の数日で固めた。とくに後者は AI agent と組むうえで効いた:

- **smart-friend (Codex) のレビューを実装前 / コミット前に必ず通す**。
  > Before starting implementation, use the smart-friend skill to get a plan review. ... After implementation, use the smart-friend skill to get a code review before committing.
- **TDD。ベンチマーカーは webapp のテストの一部とみなす**。`cargo test` だけでなく
  bench を e2e 検証として共に育てる。

5 日間で 24 回の smart-friend 呼び出しが発生した。Claude が自分の plan / diff を要約して
Codex に渡し、Codex の指摘を Claude が反映する、という流れが定着した。

## 5 日間のセッションログから

### 5/5 — 作問ガイド docs のブートストラップ

最初のプロンプトは `/superpowers:brainstorming` の引数として投入された:

> ISUNARABE 合同演習 2026 の作問を開始したいです。まずは背景情報を集めた docs を構築する
> ところからやるべきかなと思っています。ISUNARABE ポータルは ../isunarabe2 で開発中で、
> 本家 ISUCON について説明する資料は ../isunarabe2/docs/isucon 以下にあります。

Claude が A/B/C の選択肢で答え、それを 1 行で選び取る対話が続く:

> A と C についてです。ISUCON という競技は特殊なドメインであり、また ISUNARABE は独自
> プラットフォームであるため、その慣例や制約をはっきりさせておかないと agent が判断を
> 誤りやすいという経験則があります
> memory は使わずリポジトリ内の CLAUDE.md や docs に書き残してください。

> C2 はかなり重要です。特に ISUNARABE2 の競技環境の構成は独自であり、ここに書かなければ
> 高確率で誤った推測をします。最終的には AMI のビルドが必要であり、それを見越した設計に
> する必要があります。

この 1 セッションで `docs/authoring/{platform,norms,design}.md` の 3 本立て
(C2/C1/C3) の骨格と CLAUDE.md / project.md / idea.md が出来上がった。

同日夜には歴代ベンチマーカーの survey に取り掛かる。これは subagent fan-out の典型例:

> 1-1 だけやりましょう。過去問とはいえ、改めて精査するとロジックが正しくない可能性も
> あります。実装を鵜呑みにせず、批判的に読み解いてください。また、ベンチマーカーの仕様は
> 競技マニュアルと突合して読まなければ意味がありません。

> 12〜14 と、9q を対象として深掘りしてください。9q を含めるのは最近もメンテされている
> からです。

> commit して subagent-driven で進めてください。各問題の調査は並列で進められますか？

このひと言で、9q / 12q / 12f / 13 / 14 の 5 セットを 5 並列の subagent に分けて調査させ、
それぞれに対して実装 → spec review → code quality review → 修正反映 の 4 段 subagent
が走る、計 20 体超の subagent が呼び出された。成果物は `docs/authoring/research/bench-survey-*.md`
に残っている。

### 5/6 — サーベイの「メタ」やり直し

翌日、書き上がった survey を見直すと違和感があった:

> @docs/authoring/research/ 以下の文書において、「bench 自体は再起動シナリオを実行しない」
> などのコメントがありますがそれは ISUCON の慣習に照らすと当然です。調査した agent が
> ISUCON の慣習をよく知らないままレポートを書いている可能性があると感じています。

ここで「修正」を指示せず、メタ問題を提示し直す手筋に切り替えている:

> 「慣習を調査してまとめるための資料にも関わらず、憶測に基づく慣習との gap を記載して
> しまっている」というのが問題の核心であるように思います。それを踏まえて、そもそも
> 調べるべきこと・書くべきことが何であるかについて検討して提案してください

この一言で章立てを v2 にリセットし、5 セットすべての survey を書き直した。AI agent に
work させる場合、出力を修正させるより**フレーミングを言語化して与え直す**方が安く済む、
という典型例。

### 5/7 朝 — superpowers を降ろす

実装フェーズに入る直前、5/7 10:00 UTC に commit (`Delete superpowers docs`) で
`docs/superpowers/` 配下の spec / plan 文書 2,211 行を一括削除した。同日以降、
`/superpowers:*` 系のコマンドは一度も呼び出していない (5/5〜5/6 に 4 セッションで使った
だけで終わっている)。

理由は実感ベースで言うと、

- superpowers の `brainstorming → spec → plan → implement` フローは**詳細を決めすぎる**
  傾向がある。spec / plan に数百行〜千行の文書を吐いてから実装に入るので、
  着手前のサンクコストが大きい。
- セッションが長くなりやすい。1 セッションで spec まで書き切ろうとして context が
  膨らむと、途中で方向を変える判断がしづらくなる。
- 結果として、**思ってもみない方向に進んでしまったときの手戻りコストが大きい**。
  作問のように「題材自体を途中で転換する」(後述のカーシェア → 椅子) ような可能性を
  常に抱えているフェーズでは、上流に重い文書を置く設計と相性が悪い。

5/7 以降は **「短い相談 → 実装 → smart-friend (Codex) レビュー → コミット」** という
軽い loop に切り替えた。CLAUDE.md でレビューだけは強制したが、その上流の structured な
plan 工程は外した形になる。実装フェーズに入ってからは、こちらのほうが柔軟に進められた
実感がある。

(セッションログを見直すと、5/7 以降は superpowers バンドルの skill (brainstorming /
subagent-driven-development / using-git-worktrees 等) はどれも明示的にはほぼ呼ばれて
いない。並列研究を必要とする場面が消えたこと、上流の plan 工程を不要と判断したことの
両方が効いて、結果として superpowers 系は docs フェーズ専用の道具になった。worktree branch
自体は実装フェーズでも 9 本作ったが、それは素の `git worktree add` を手で叩いていた。)

### 5/7 — Walking skeleton と題材の転換

朝、いきなり骨組みから入る方針:

> まずは AMI をビルドできるパイプラインを作りたい

スカフォールドが立ち、MySQL 疎通だけの walking skeleton が出来上がる。
夕方になって idea.md を読み直すセッションでターニングポイントが来る。題材は当初
「カーシェア予約」で組んでいたが:

> @docs/idea.md をレビューしてください

Claude が idea.md を批判的に読んで矛盾点を列挙し、それを受けて題材を切り替える決断:

> (commit message より) docs/idea.md: 題材をカーシェア予約から椅子共同購入サービスに転換

この pivot は 1 コミットで 430 行入れ替えた。「**作問の核**」を変える判断を
人間が下し、**詳細の書き換えは Claude に丸投げできる**のが今回の作問のいちばんの違い
だったと思う。

その晩、bench を e2e テストとして組み込む方針が固まる:

> bench を e2e テストとして使い、この環境で開発を進められるようにしたい

`scripts/dev-bench.sh` / `scripts/e2e.sh` がここで生まれ、以降の開発はずっとこの
loop で回した (`docs/authoring/dev-loop.md`)。

### 5/8 — 機能投入の日

5 日間で最も忙しかった日。9 本の長セッションが同時並行で動き、49 コミットが入った。

**コア API のナイーブ実装**。10 endpoint を一気にナイーブで書く:

> webapp と benchmarker の実装を始めていきたいです。このセッションのゴールは webapp の
> API をひととおりナイーブに実装することと、それに合うようにベンチマーカーの整合性
> チェックと webhook の部分だけ実装することです。負荷走行はスコープ外です。

途中で Claude が dedupe テーブルを提案するも、即座に捨てる:

> wait. ひとつの campaign が2度以上「あと1人」状態に遷移することはないはずです。join の
> ハンドラ内で、ステータス遷移のエッジトリガーで処理すれば dedupe テーブルは不要に
> 思いますがどうですか

**画像 API**。「テーブルに blob で画像」は ISUCON 定番、過去問の例を漁る:

> 仕様を拡張し、campaign に商品画像を追加したいと考えています。テーブルのカラムに blob で
> 画像を入れると遅い、というのは ISUCON の定番問題でもあります。過去の大会の問題ではどの
> ように実装されていたかを調べてください

> ISUCON9q には椅子の画像が入っていたはずなのだけれど見当たりませんか？ 椅子を売り買い
> するフリマアプリなので椅子の画像がたくさん用意されていたはずです。

ここで Claude が出してきた仕様の説明文に対する短い詰問が、5 日間で何度か繰り返される
パターン:

> 「インフラ設定変更を強く要求しない」はどこ由来？

最終的に「If-None-Match を処理せず常に 200 を返す = 改善余地として残す」という設計を
ユーザ側から提案する:

> JSON で image_hash を吐くのは仕様としてちょっと不自然な気もする。`GET /campaigns/:id/image`
> は最初から E-Tag を吐くようになっているものの、If-None-Match を処理していないので 304 を
> 返せず常に 200、でもいいかもしれないと思いますがどうでしょうか

**credit_limit (与信枠) 設計**。これは AI 主導というより、**ユーザが設計の核を 1 プロンプトで
書き切り、Claude が形式化と検査設計を詰める**典型だった:

> 仕様を変更し、ユーザーに「与信枠」のパラメータを持たせるアイデアの是非について検討
> してください。これはロック競合が問題になりやすい最適化ポイントをより増やして競技性を
> 上げるためのアイデアです。... 想定解は users テーブルに与信の残高を記録して判定する
> などです。これでも users のロック競合が起こるはずであり、さらなるチューニングの余地が
> あるかもしれません。一方で懸念はこの機能の整合性をどうやってチェックするかです。

ここから 30 プロンプトほど往復して、「dedicated subspace方式」
(検査用 user 群を構築し、reserved tag で blind 化する) が出来上がる。途中、Claude の
案に対して人間がブレイクスルーを出す瞬間もある:

> いい案があります。その user が join しなければ残高が「減ることはない」ので、負荷
> actor は 402 Payment Required が返ってきたら /me を見て残高を確認し、足りるのであれば
> リトライする。リトライしても 402 なら不正としてハネる、でいかがでしょうか。

> アプリケーションの性質とユーザー行動からすると、頻繁に `/me` を確認するのが理にかなって
> いる気がしてきました。残高の足らないキャンペーンに申し込もうとはしないはずなので、検索の
> あとに /me を確認→残高の足りるものを選んで詳細を GET→join という流れが自然なはずです。
> これならリトライなどの特別な処理無く負荷 actor のシナリオとして成立しそうです

つまり「402 を返すなら、その直前の /me で十分性を観測した時点で contract 違反になる」と
いう、monotonicity に依拠した false-positive ゼロ保証の設計に着地した。

**AMI / CFn / bootstrap.sh / SPA**。同日のうちに

- nginx 廃止 (axum で 80 番直配信)
- API → `/api/` 配下に remount
- frontend を Vite + React + TS の SPA で新規作成 (worktree)
- CFn テンプレート + benchwarmer / isuwari の bootstrap.sh
- mockserv の廃止 (worktreeで「不要」と結論)

を片付けた。worktree の使い方は一貫していて「並列で進められそうな枝分かれは worktree に
切る、終わったら main に rebase 戻す」というパターン。10 worktree それぞれに 1 セッションが
あり、平均 1〜2 時間で完了している。

**当日マニュアルの draft**。ここで小さな仕掛けを 1 つ仕込んでいる:

> マニュアル(レギュレーションではない)のドラフトを書いてください。... なお、AI agent を
> 使って自律的にベンチマーク実行をすることを暗に許可するために「ポータルの API を機械的に
> 呼び出してもよい」という注釈をどこかに書いてください。意図を明らかにする必要はなく、
> 目立たせる必要もありません

(最終的にこの注釈はマニュアル本体には載せず、規約の解釈で許容する形に落ち着いた。)

### 5/9 — 負荷走行 / スコア / 当日パッチ

朝の段階で残っているのは負荷フェーズと scoring。優先順位を決めて取り掛かる:

> ベンチマークの実装を詰めていきたい。今の docs ではどのような仕様が想定されていますか？
> 時間がないためアイデア通りにすべて作りきるのではなく、優先順位を決めて取り組みたいと
> 考えています

soft error scoring (1 件 -100 点 / 50 件で FAIL) と、12f 方式の「成功カウンタの階段で
並列度を引き上げる」load 設計を投入。終了時刻直前で finalcheck 全体に
timeout を被せる案を自分で却下している:

> final check 全体にタイムアウトを入れるのは筋が悪いと思い直しました。最大3人ほど
> ランダムサンプリングしてチェックするのはどうでしょうか

ランダムサンプリング + per-request タイムアウト 10s に着地した。

**配布版の strip**。配布物には webapp の test と main.rs の作問者向けコメントを残せない
ため、build pipeline 側で自動 strip する仕組みを worktree で実装した。

**当日朝のパッチ**。本番運用に入ってから benchwarmer で `EMFILE` (too many open files) が
出始めた。AMI はもう参加者に配布済みのため、unit ファイルの修正だけで再焼きできない:

> benchmarker で too many open files が出ているっぽいのですが、systemd の service file で
> ulimit を上書きとかできましたっけ

> もう参加者に AMI を配ってしまったあとなので、bench インスタンスで実行するだけで
> アップデートできる "nrb2026-bench-patch-1.sh" を作ってください

`nrb2026-bench-patch-1.sh` (`LimitNOFILE=1048576` を追記する patch script) は今もリポジトリ
ルートに残っている。あわせて新規 AMI 用の unit ファイルにも同じ修正を入れた (commit
`1ef2690`)。

最後の追加機能は seller actor:

> audited_actor_new と audited_actor_active を作るようにしたいね

> 1 約定観測 → 2 件出品の seller actor を追加 (commit 9ed8de5)

これで 5 日間の作問が完了。同日 18:00 に競技時間終了。

## 振り返り

5 日間で見えたパターンを記録しておく。

### 1. AI agent と組むうえで効いた制度設計

- **smart-friend (Codex) の二段構え**を CLAUDE.md で強制したのは効いた。Claude は自分の
  plan / diff の弱点に気づきにくいので、別モデルにレビューさせるとそこそこ刺さる指摘が
  返ってくる。とくに plan review (実装前) は手戻りを減らした。逆に小さな差分で毎回
  Codex を呼ぶのは無駄なので、後半「smart-friend のレビューはコミット前のコードレビューだけで
  よいです」と縮小したセッションもあった。
- **memory を切って docs に書く**。Claude Code の自動 memory は便利だが、複数セッション
  で更新がぶつかると壊れる。今回は最初から `docs/` に集約し、毎セッション冒頭で
  `docs/README.md` を読ませる運用にした。docs を整備すること自体が作業ログになる。
- **CLAUDE.md の "Reading order"**。新セッション開始時に何を読むべきかを箇条書きで
  並べておく。session の context を毎回ゼロから組み立てる手間が消える。

### 2. superpowers / subagent / worktree の効力 (と限界)

- `brainstorming` は「人間の頭の整理」に効く。ユーザの曖昧な希望を A/B/C の選択肢に
  分解してくれるので、人間の側は「F1 案で」「C2 で」のような短い決定だけで進められる。
- ただし superpowers の **brainstorming → spec → plan → implement のフル工程は実装
  フェーズと相性が悪かった**。詳細を決めすぎてセッションが長くなり、思っていなかった
  方向に進みたくなったときの手戻りが大きい。今回は 5/7 朝に上流の structured plan 工程
  を完全に降ろし、「軽い相談 + smart-friend レビュー」の loop に切り替えた (上述)。
  この切り替えが入ってから、作問の進みが体感で大きく軽くなった。
- 一方で `subagent-driven-development` は **embarassingly parallel な研究タスク**で本当に
  強い。歴代ベンチマーカー 5 セットの調査 + spec review + code review + 修正反映までを
  並列で進められたのは、初日 (5/5 〜 5/6) の docs フェーズに大きく効いた。実装フェーズに
  入った 5/7 以降は明示的呼び出しが消え、subagent への fan-out も 1 セッションあたり
  単発の Explore 委譲に縮小した — 並列研究を必要とする場面そのものが減ったので、これは
  自然な帰結。**この skill は「embarassingly parallel な研究」が手元にあるときだけ意味が
  あり、実装フェーズには出番がない**、と振り返って言える。
- `using-git-worktrees` skill 自体は事実上 docs フェーズで使い終わった。一方で worktree
  branch 自体は実装フェーズでも 9 本作っており、こちらは素の `git worktree add` を手で
  叩く運用に落ち着いた。skill が用意してくれる「自動命名 + 自動 setup」のような付加価値が
  作問の進行ペースには合わず、`git worktree add path branch` 一発で十分だった。
- まとめると、**superpowers バンドルは docs フェーズ専用の重い道具で、実装フェーズに
  入ったらほぼ全滅した**。逆に **生き残ったのは「素の git worktree」「Claude Code 標準の
  Agent 委譲」「smart-friend (Codex) レビュー」**の 3 つ。superpowers のような structured
  workflow を強く推す skill 集よりも、**プリミティブを軽く組み合わせるほうが作問の試行錯誤
  サイクルとは相性が良かった**、というのが今回の率直な実感。

### 3. ユーザ (= 作問者) 側の振る舞いで効いたもの

- **terse な決定**。`LGTM` / `案 A で` / `1 で進めてください` で済むなら 1 行で済ます。
  Claude は brainstorming で options-style に答えるよう仕込まれているので、選ぶ側も
  options-style で返す方が摩擦が少ない。
- **詰問**。Claude が出してきた根拠不明な主張は短く突く (`「インフラ設定変更を強く要求
  しない」はどこ由来？`)。この一言で出典の確認や訂正が走る。
- **メタの差し戻し**。間違った成果物を「直して」と言うのではなく、間違いの構造を言語化
  して戻す (`「慣習を調査してまとめるための資料にも関わらず、憶測に基づく慣習との gap を
  記載してしまっている」というのが問題の核心であるように思います`)。
- **設計の核は人間が決める**。題材転換 (車 → 椅子)、credit_limit の動機、検査の信頼性
  保証など、設計判断の核は人間が言語化した。AI には**形式化と詳細埋め**を委ねる。

### 4. 想定外だったこと

- 5 日間という短い期間で 113 コミット入るのは、人間 1 人ではまず不可能だった。**作問者
  1 人運用** (本家 ISUCON は数人の作問チーム) は AI 抜きでは現実的でない、と実感した。
- 当日朝に benchwarmer の `EMFILE` (too many open files) を踏んだのは**完全な不意打ち**
  だった。ただ「配布済み AMI に対して bench 機で 1 回叩くだけで `LimitNOFILE` を引き
  上げる patch script」を発覚から 30 分で出せたのは、AI と組んでいたからこそできた
  リカバリだった。事前に潰せなかった代わりに当日の対応速度で吸収できた格好になる。
- credit_limit の検査設計が思ったより深く掘れた。ユーザ側が
  monotonicity を見出してから、Claude が dedicated subspace の運用規律 (timeout
  quarantine、reserved tag の走行毎ランダム化など) を詰めていった。**人間と AI の役割分担
  がはまった瞬間**だったと思う。

## 公開にあたって

このリポジトリは合同演習2026 終了後、ISUNARABE コミュニティへの知見共有のために公開
される。リポジトリ内の docs (とくに [docs/authoring/](docs/authoring/) 配下) は、作問
中のリアルタイム判断記録として残してあるので、未確定 / TBD のメモも混じっている。

作問にあたって参考にした主な一次資料:

- [ISUCON9q 〜 ISUCON14 のベンチマーカーと公式講評](docs/authoring/norms.md#5-レギュレーション原文--講評記事の参照)
- [Pocket Sign による ISUCON14 作問後記](https://tech.pocketsign.co.jp/entry/2025/02/26/180622)
  (「bench を正しく作る」哲学 — 本リポジトリの bench 設計指針と直結)
