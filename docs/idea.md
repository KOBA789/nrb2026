# ISUNARABE合同演習2026 問題設計メモ

## コンセプト

「ジュニアなエンジニアがvibe codingで作った椅子共同購入サービスがやたらと遅い。8時間でなんとかして！」

AI時代ならではの問題構造として、過剰にサービス分割された大きなコードベースを、AIエージェントの力で書き直し・統合することが有効な戦略になる。従来のISUCONでは非現実的だった「書き直し」が、AIによって合理的な選択肢になるというメッセージ。

## サービス概要

椅子グループ購入サービス。

椅子が出品され、ユーザーが参加して、一定数の参加者が集まると購入が確定する。ユーザーは自分の興味に合った椅子をタグで検索できる。ユーザーは自分が参加しているキャンペーンの購入確定後に課金される。

current_count == goal_count になったキャンペーンは募集終了=約定 (status: closed) となり、参加者は即座に（同一トランザクションで）課金される。ユーザーは自分が参加しているキャンペーンの状態を確認できる。

ユーザーには **与信枠 (credit_limit)** が設定されており、現在参加中で未約定の campaign の price 合計がこの枠を超えるような新規参加はできない (与信不足での 402 拒否)。約定 (close) によって枠は回復する。これにより、人気キャンペーンへの並列 join が **ユーザー単位の更新競合** を生むようにし、ロック競合の最適化を競技テーマとして加える。

ユーザーは検索条件を保存できる。保存された検索条件にマッチするキャンペーンが残り1人で募集終了となったとき、ユーザーに通知される。仕様としては通知は optional であり、実装しなくてもよい。初期実装（配布版のナイーブ実装）では非効率な実装とする。ただし、実装すると約定が増え、スコアに貢献する。

## スコアリング

- 約定したキャンペーンに参加していたユーザー数 × 1000点
- ソフトエラー (per-request 10s タイムアウト / status 不一致 / レスポンス形式違反 / 画像系の挙動不一致など) 1 件あたり -100 点。50 件到達で FAIL。係数・閾値は仮。
- critical エラーによる中断 (即 0 点で終了)。critical エラーは以下:
  - 二重課金 (1 participant に対し charges が2件以上)
  - 通知の重複送信 (同じ (user_id, campaign_id) ペアの webhook が2回以上届く)
  - closed → open の状態逆転
  - goal_count 超過参加 (current_count > goal_count)
  - 課金漏れ (closed なのに participants の charges が無い)
  - 与信枠超過観測 (GET /api/me で credit_used > credit_limit が観測された)
  - 与信不足 join 成功 (bench が dedicated 検査空間で `before_credit_used + price > credit_limit` と確信した join に対し、webapp が 200 を返した)
  - 与信契約違反 (GET /api/me で credit_used + P ≤ credit_limit を確認した直後、同 user が他に join を発行していない期間内に POST /api/campaigns/{c}/join (price=P) が 402 を返した。409 (race による close / 既参加) は許容)

- 画像系の critical (上記とは別枠): **bench が POST 成功レスポンスで受け取った campaign_id**
  に対してのみ critical 化する (= 自前 campaign のみ false positive 回避):
  - `GET /api/campaigns/{id}/image` の body bytes が POST した bytes と不一致
- 画像系の **soft (= 改善対象 / 減点扱い、score 中断にはしない)**:
  - `Content-Type ≠ image/jpeg`
  - `ETag` 欠落・形式違い (= body の SHA256 を `"<hex>"` で囲んだ値であるべき)
  - `If-None-Match` 完全一致で 304 が返らない (= 配布版では未実装、競技者の改善対象)
  - 画像系 negative probe (400 / 401 / 404 / 413) の期待外れ

1ユーザーが参加した複数 campaign が closed になった場合、そのユーザーは campaign 数だけ加点される。

## データモデル

- users
  - id (UUID v4)
  - name
  - credit_limit (与信枠の上限。POST /api/users で **60000** が初期値として設定される。全 user 共通の固定値)
  - created_at (JSON には含めない)
  - (credit_used は仕様上の派生値。定義: `Σ price[c] for c in {自分が campaign_participants にいる campaigns where current_count < goal_count}`。`charges` の有無で判定しない (= close 直前の race を排除した定義)。初期実装(ナイーブ実装)では都度計算、選手の最適化余地として users 行に materialize 可能)

- campaigns
  - id (UUID v4)
  - name
  - description
  - price
  - goal_count
  - image (LONGBLOB)
  - created_at

- campaign_participants
  - id (UUID v4) (JSON には含めない)
  - campaign_id
  - user_id
  - created_at

- charges
  - id (UUID v4)
  - campaign_participant_id
  - created_at (ソートで使うし JSON にも含める)

- campaign_tags
  - campaign_id
  - tag_id
  - created_at (JSON には含めない)

- tags
  - id (UUID v4)
  - name
  - created_at (JSON には含めない)

- saved_searches
  - id (UUID v4)
  - user_id
  - created_at (JSON には含めない)

- saved_search_tags
  - saved_search_id
  - tag_id
  - created_at (JSON には含めない)

## API 共通仕様

### 認証

認証はテーマではないので、ユーザーIDをリクエストヘッダに含めるものとする。

headers:
```
X-User-ID: 90071585-b35d-4f5c-aa18-b5a0071526be
```

### エラー

- X-User-ID が必要な API でヘッダなし: 401
- X-User-ID のユーザーが存在しない: 401
- JSON が不正: 400
- 存在しない campaign_id: 404
- 存在しない tag: 400
- 画像 (image) が不正: 400
  (フィールド欠落 / base64 デコード失敗 / 0 byte / JPEG magic 不一致)
- 画像のデコード後サイズ > 200 KiB (= 204_800 byte): 413

### 画像 (image) の不変条件

- POST /api/campaigns で受け取る `image` は **base64 (RFC 4648 standard、canonical padding 要求)**
  でエンコードされた **JPEG バイナリ** であること。URL-safe base64 / padding 欠落は 400。
- `GET /api/campaigns/{id}/image` のレスポンス body は **decode 後と同一の JPEG bytes** であること。
  サーバ側の再エンコード / 圧縮変換 / メタデータ書き換えは禁止。
- 画像のハッシュは `GET /api/campaigns/{id}/image` の `ETag` ヘッダ (= body の SHA256 を `"<hex>"`
  で囲んだ値) で表現する。レスポンス JSON には image hash を含めない。

## API一覧

### POST /api/users

req:
```json
{
  "name": "ユーザー名" // 1文字以上100文字以下。重複検査無し
}
```

res:
```json
{
  "id": "90071585-b35d-4f5c-aa18-b5a0071526be",
  "name": "ユーザー名",
  "credit_limit": 60000
}
```

### GET /api/me

認証必須。ユーザーの現在の与信状況を返す。

res:
```json
{
  "id": "90071585-b35d-4f5c-aa18-b5a0071526be",
  "name": "ユーザー名",
  "credit_limit": 60000,
  "credit_used": 18000
}
```

- `credit_used` は「ユーザーが参加中で未約定の campaign の price 合計」。初期実装(ナイーブ実装)では他の値から計算して返す。選手の最適化余地。
- **不変条件**: 同 user が新たな join を発行していない期間中、`credit_used` は単調非増加 (refund のみが起きうる)。この性質はベンチマーカーの整合性検査が前提として依拠するため、実装は壊してはならない。
- `credit_used > credit_limit` が観測されたら critical (与信枠超過)。

### GET /api/campaigns

query:
| パラメータ | 説明 |
| --- | --- |
| tags | タグのリスト。カンマ区切りで3つまで指定可能。AND 検索になる。例: `タグ1,タグ2`。同じものを重複して指定したら 400 Bad Request を返す |
| sort | new: created_at の降順 (デフォルト), active: 最近の活動順。ソートキーは last_joined_at が非 null ならその値、null (= 参加者0人) なら created_at。降順。参加者0人の campaign も結果に含まれる |

res:
```json
[
  {
    "id": "a0ed477b-cbca-4611-8a68-1eb1caf7380d",
    "name": "ナイスな椅子、ナ椅子",
    "description": "この椅子はとてもナイスで、座り心地が最高です",
    "price": 10000,
    "goal_count": 10,
    "current_count": 5, // 初期実装(ナイーブ実装)では他の値から計算して返す。選手の最適化余地
    "tags": ["nice", "comfortable"],
    "status": "open", // 検索結果には open のみ含まれるものとする。初期実装(ナイーブ実装)では他の値から計算して返す。選手の最適化余地
    "created_at": "2024-05-01T12:00:00.000Z",
    "last_joined_at": "2024-06-01T12:00:00.000Z", // 参加者が最後に増えた日時。参加者がいない場合は null。初期実装(ナイーブ実装)では他の値から計算して返す。選手の最適化余地
    "participants": [
      // joined_at asc でソートされているものとする
      {
        "user_id": "ユーザーID",
        "name": "ユーザー名",
        "joined_at": "参加日時"
      },
      ...
    ]
  },
  ...
]
```

件数は最大30件とする。常に participants.length == current_count でなければならない。また status == "closed" なら current_count == goal_count == participants.length でなければならない。
画像 (image) は本レスポンスには含めず、`GET /api/campaigns/{id}/image` で個別に取得する。

### POST /api/campaigns

req:
```json
{
  "name": "ナイスな椅子、ナ椅子", // 1文字以上100文字以下
  "description": "この椅子はとてもナイスで、座り心地が最高です", // 1文字以上1000文字以下
  "price": 10000, // >=2000, <=20000
  "goal_count": 10, // >=2, <=20
  "tags": ["nice", "comfortable"], // 10個まで指定可能。予め存在するタグでなければならない。
  "image": "<base64-encoded JPEG>" // 必須。RFC 4648 standard / canonical padding。
                                   // decode 後 ≤ 200 KiB / JPEG magic (FF D8 FF) 必須。
}
```

バリデーション順序: 既存スカラ (name → description → price → goal_count) → tags 件数 / 名前重複
→ image (base64 デコード / サイズ / magic) → tags の DB 解決。

res: (201 Created)
```json
{
  "id": "a0ed477b-cbca-4611-8a68-1eb1caf7380d",
  "name": "ナイスな椅子、ナ椅子",
  "description": "この椅子はとてもナイスで、座り心地が最高です",
  "price": 10000,
  "goal_count": 10,
  "current_count": 0,
  "tags": ["nice", "comfortable"],
  "status": "open",
  "created_at": "2024-05-01T12:00:00.000Z",
  "last_joined_at": null,
  "participants": []
}
```

campaign 作成者の自動 join はされない。時間切れなどによる終了などはない。
レスポンスに image は含まれない。アップロードされた画像は `GET /api/campaigns/{id}/image` で取得する。

### GET /api/campaigns/{campaign_id}

res:
```json
{
  "id": "a0ed477b-cbca-4611-8a68-1eb1caf7380d",
  "name": "ナイスな椅子、ナ椅子",
  "description": "この椅子はとてもナイスで、座り心地が最高です",
  "price": 10000,
  "goal_count": 10,
  "current_count": 5, // 初期実装(ナイーブ実装)では他の値から計算して返す。選手の最適化余地
  "tags": ["nice", "comfortable"],
  "status": "open", // open: 参加募集中, closed: 募集終了。初期実装(ナイーブ実装)では他の値から計算して返す。選手の最適化余地
  "created_at": "2024-05-01T12:00:00.000Z",
  "last_joined_at": "2024-06-01T12:00:00.000Z", // 参加者が最後に増えた日時。参加者がいない場合は null。初期実装(ナイーブ実装)では他の値から計算して返す。選手の最適化余地
  "participants": [
    // joined_at asc でソートされているものとする
    {
      "user_id": "ユーザーID",
      "name": "ユーザー名",
      "joined_at": "参加日時"
    },
    ...
  ]
}
```

### GET /api/campaigns/{campaign_id}/image

認証必須。campaign に紐づく画像 (JPEG) を返す。

| 条件 | レスポンス |
|---|---|
| 不在 id | 404 (空 body) |
| その他 | 200, body = JPEG bytes, `Content-Type: image/jpeg`, `ETag: "<SHA256(body) hex>"` |

- `ETag` は body の SHA256 を 64 文字 lowercase hex で表したものを `"` で囲む (= strong ETag)。
- **`If-None-Match` は配布版では処理しない**。条件付き GET (304) の実装は競技者の改善余地。
- `Cache-Control` は付与しない (= 同じく改善対象)。

### POST /api/campaigns/{campaign_id}/join

エラー:
- 409 Conflict with empty body — いずれかが該当:
  - すでに参加済み
  - campaign が open でない(status = closed)
- 402 Payment Required with empty body — 与信不足:
  - **pre-check 固定**: 当該 join 処理開始時点 (= 自分自身の participant 行追加 / 自分の close による refund 反映 より前) の credit_used を `before_credit_used` とし、`before_credit_used + price > credit_limit` なら 402。
  - 最後の 1 人として join → 同一 transaction で close になるケースでも、close による自分自身の refund を **先取りせず** に判定する (= 「join 後すぐ refund されるから実質残高は足りる」という解釈は採らない)。実装の解釈差を排除するための仕様固定。
- 成功: 200 OK

判定順序: **409 系 (closed / 既参加) を先に判定し、その後で与信判定 (402)** を行う。これは「campaign の状態 / 参加重複」が「与信」より優先することを保証し、bench 側が状態遷移を期待するシナリオで予測可能なレスポンスを得られるようにするため。

req:
```json
{}
```

res:
```json
{
  "id": "a0ed477b-cbca-4611-8a68-1eb1caf7380d",
  "name": "ナイスな椅子、ナ椅子",
  "description": "この椅子はとてもナイスで、座り心地が最高です",
  "price": 10000,
  "goal_count": 10,
  "current_count": 10,
  "tags": ["nice", "comfortable"],
  "status": "closed",
  "created_at": "2024-05-01T12:00:00.000Z",
  "last_joined_at": "2024-06-01T12:00:00.000Z",
  "participants": [
    // joined_at asc でソートされているものとする
    {
      "user_id": "ユーザーID",
      "name": "ユーザー名",
      "joined_at": "参加日時"
    },
    ...
  ]
}
```

自分が最後の1人だった場合、参加と同時に campaign の status が closed になり、参加者全員に課金が発生する。

closed なレスポンスが返ったとき、ベンチマーカーは GET /api/charges を呼び出して課金が正しく発生しているかを確認する。
大量の join をすると GET /api/charges が巨大化してしまうので、GET /api/charges を確認するユーザーは専用のペルソナを与えるべきかもしれない(TBD)。

current_count == goal_count - 1 への遷移時点で、保存された検索条件にマッチするユーザーに通知が送られる。1人のユーザーは、同じ campaign について通知を高々1回しか受け取らない。同じユーザーに2度送るのは迷惑なため、critical な整合性エラーとする。通知は送信されなくてもいいし、遅延は許容される。1人のユーザーが同じ campaign について複数の saved_searches でマッチする場合でも、通知は1回だけ送るものとする。

### POST /api/saved_searches

ユーザー1人につき、10件の保存された検索条件を持てるものとする。
上限に達している場合は、409 Conflict を返す。削除 API はないものとする。

req:
```json
{
  "tags": ["タグ1", "タグ2"] // 3個まで指定可能。AND 検索になる。予め存在するタグでなければならない。同じものを重複して指定したら 400 Bad Request with empty body を返す
}
```

res: (empty body, 201 Created)

### GET /api/charges

全件返す。ソートは created_at の降順。

res:
```json
[
  {
    "id": "ee88614a-92eb-457f-9d12-5c124d32f1f2",
    "amount": 10000,
    "campaign": {
      "id": "a0ed477b-cbca-4611-8a68-1eb1caf7380d",
      "name": "ナイスな椅子、ナ椅子",
      "price": 10000
    },
    "created_at": "2024-06-01T12:00:00.000Z"
  },
  ...
]
```

### GET /api/tags

res:
```json
[
  "タグ1",
  "タグ2",
  ...
]
```

ベンチマーカは順序を気にせず、集合の一致で確認する。

### POST /api/initialize

認証不要。

この API はベンチマーク走行開始時に1回だけ呼び出される。これを呼び出すと、データベースが初期化され、seed データが投入される。30秒以内に完了する必要がある。（タイムアウトすると0点）

通知を送信する先の webhook URL を受け取る。残り1人で募集終了となったときの通知に使う。

req:
```json
{
  "notification_webhook_url": "http://example.com/webhook"
}
```

res: (200 OK)
```json
{}
```

## 通知サービス仕様

ベンチマーク走行時はベンチマーク自身がこのエンドポイントを提供し、挙動のバリデーションをする。

### POST /webhook

500を返した場合でも webapp はリトライすべきではない。

req:
```json
{
  "type": "campaign_closing_soon",
  "user_id": "90071585-b35d-4f5c-aa18-b5a0071526be",
  "campaign": {
    "id": "a0ed477b-cbca-4611-8a68-1eb1caf7380d",
    "name": "ナイスな椅子、ナ椅子",
    "description": "この椅子はとてもナイスで、座り心地が最高です",
    "price": 10000,
    "goal_count": 10,
    "current_count": 9,
    "tags": ["nice", "comfortable"],
    "status": "open",
    "created_at": "2024-05-01T12:00:00.000Z",
    "last_joined_at": "2024-06-01T12:00:00.000Z"
  }
}
```

`user_id` は通知の宛先となるユーザーの ID。ベンチマーカは `(user_id, campaign.id)` ペアで重複検査を行う。

webhook payload にも `image` / hash 系のフィールドは含まない (campaign の画像取得は
`GET /api/campaigns/{id}/image` を介してのみ)。

res: (empty body, 201 Created)
