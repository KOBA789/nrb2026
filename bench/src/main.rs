//! nrb2026 bench (整合性 + webhook 受信 + load phase)
//!
//! 仕様: docs/idea.md  (スコア式 / critical 一覧)
//! 設計: docs/authoring/design.md §4.1 (与信検査の Phase 1 構造)
//!       docs/authoring/norms.md §3 (ISUCON 14 軽量 world model + 12q Validate worker のハイブリッド)
//!
//! Flow: init → pretest → negative_probes → integrity_scenario
//!       → run_load_phase (audited + notification actors, 固定時間 + 12f 方式の ramp)
//!       → finalcheck (charges 突合)
//!       → REPORT_FD に scorejson `{"score":N}\n`
//!
//! Score:
//!   * critical 0 件 → score = score_total (= Σ closed campaigns × participants × 1000)
//!   * critical >= 1 件 → score = 0 (= 走行中断、既存の慣習)
//!
//! env (benchwarmer 由来 + production override):
//!   REPORT_FD / ISUXBENCH_REPORT_FD : 数値 fd
//!   BENCHWARMER_target_ip           : webapp の IP (default 127.0.0.1)
//!   WEBAPP_BASE_URL                 : webapp の base URL。優先される。
//!                                     未設定時は "http://{BENCHWARMER_target_ip}:{WEBAPP_PORT}"
//!   WEBAPP_PORT                     : webapp の listen port (default 8080、production は 80)
//!   WEBHOOK_URL                     : webapp に渡す webhook URL
//!                                     (default `http://{BENCHWARMER_target_ip}:9999/webhook`)
//!   BENCH_LOAD_DURATION_SECS        : load phase の固定走行時間 (default 60)
//!   BENCH_AUDITED_NEW_ACTORS_MAX    : sort=new actor の上限 (default 3)
//!   BENCH_AUDITED_ACTIVE_ACTORS_MAX : sort=active actor の上限 (default 5)
//!   BENCH_NOTIFICATION_ACTORS_MAX   : webhook 駆動 join actor の上限 (default 4)
//!   BENCH_INITIAL_ACTORS_PER_KIND   : load 開始時に各 actor 種を何体 spawn するか (default 1)
//!   BENCH_RAMP_STEP_JOINS           : 何件 join 成功で各 actor 種を +1 体 spawn するか (default 5、min 1)

use axum::{body::Bytes, extract::State, http::StatusCode, routing::post, Router};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Digest as _;
use std::collections::{HashMap, HashSet};
// seed の UUID / tag 名 / 件数は seed-data crate から引く (= seed_gen と同一の
// single source of truth)。bench に hardcode しない。
use std::env;
use std::io::Write;
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

// === scoring constants (docs/manual.md §6.3 / §6.5 / §6.6) ===

/// 通常リクエストの per-request タイムアウト。超過は soft error 1 件として計上する。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// /api/initialize のみ別枠 (= 30 秒、超過は即時 FAIL)。
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
/// finalcheck で /charges を引いて突合する unique user の最大サンプル数。
/// 全件直列だと wall-clock が unique users × per-req timeout に比例して伸びるので、
/// ランダムサンプリングで上限を切る (= 走行全体の wall-clock を予測可能にする)。
/// 同時に「確実な検出」から「分散検出」への切替えでもあり、検出はサンプル運に依存する
/// 仕様にしている (= 期待される選手実装は missing/double charge を出さない)。
const FINALCHECK_SAMPLE_SIZE: usize = 3;
/// soft error 1 件あたりの減点。負のスコアは 0 にクランプ。
const SOFT_PENALTY: i64 = 100;
/// soft error 件数の FAIL 閾値。到達した時点で走行を打ち切り score=0。
const SOFT_FAIL_THRESHOLD: u64 = 50;

/// 走行終了時の最終スコア計算。critical>=1 または soft>=50 で 0 (FAIL)、
/// それ以外は max(0, total - soft*100)。
fn compute_score(critical: usize, soft: u64, total: i64) -> i64 {
    if critical >= 1 || soft >= SOFT_FAIL_THRESHOLD {
        return 0;
    }
    let penalty = (soft as i64).saturating_mul(SOFT_PENALTY);
    total.saturating_sub(penalty).max(0)
}

// === bench state ===

#[derive(Debug, Clone)]
struct ImageFixture {
    path: PathBuf,
    hash_hex: String,
    #[allow(dead_code)]
    size: usize,
}

/// closed を初観測した campaign の participants snapshot。
/// finalcheck で各 participant の /api/charges を引いて二重課金 / 課金漏れを検査するときに使う。
#[derive(Debug, Clone)]
struct ClosedRecord {
    /// participants.user_id (joined_at asc)
    participants: Vec<String>,
}

/// observation の発生 phase。score 加算は Load 期 close のみに限定するための区別。
/// Validation 期 (pretest / negative_probes / integrity / finalcheck) の close は
/// finalcheck の charges 突合対象には入れるが、idea.md スコア式には算入しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Validation,
    Load,
}

struct BenchState {
    notifications: Mutex<HashSet<(String, String)>>,
    /// 各 campaign について bench がこれまで観測した current_count の最大値。
    /// webapp の current_count は participants 行追加のみで増える単調非減少値なので、
    /// `observation.current_count < observed_max` は stale snapshot (= 並列観測のレース)。
    /// stale 観測は critical 検査の入力にしない (= false positive 回避)。
    observed_max_count: Mutex<HashMap<String, i32>>,
    /// score 加算 / finalcheck 対象の closed campaign。初観測の participants を保存。
    /// 局所不変条件 (participants.len == current_count == goal_count) を満たすものだけ
    /// 登録される。違反は record_campaign_observation 内で critical 化して return する。
    closed_campaigns: Mutex<HashMap<String, ClosedRecord>>,
    /// 約定キャンペーンの加点合計 (= Σ participants × 1000)。
    /// critical >= 1 件のときは捨てる。0 件のときに score として吐く。
    score_total: AtomicI64,
    critical: Mutex<Vec<String>>,
    /// soft error 件数 (per-req 10s timeout や status/形式不一致など、docs/manual.md §6.5)。
    /// 計上は add_soft の fetch_add で原子的に行い、SOFT_FAIL_THRESHOLD (50) に到達した
    /// 1 件目だけ cancel_global.cancel() を発火する (= 競合下でも cancel は冪等で多重発火しない)。
    soft_count: AtomicU64,
    /// 走行全体のキャンセル token。50 件 soft 到達で発火し、load actor を即時停止させ
    /// finalcheck も冒頭 guard で skip する。run_load_phase の deadline 用 token は
    /// child_token() でこの下に紐づける (= 親側の 50-soft cancel が child へ伝播)。
    cancel_global: CancellationToken,
    /// 起動時にスキャンした fixture 一覧 (path + 期待 SHA256 hex)。
    fixtures: Vec<ImageFixture>,
    /// bench 自身が POST した campaign の id → 期待 fixture SHA256 (lowercase 64 hex)。
    /// seed 由来の campaign は登録しない (= GET image の body 比較 critical を自前のみに限定)。
    posted_campaigns: Mutex<HashMap<String, String>>,
    /// fixture ピック / 検査用の決定的 PRNG。
    rng: Mutex<StdRng>,
    /// notification 駆動 join のための user 別 channel sender。
    /// webhook handler が user_id をキーに try_send(campaign_id) し、対応 actor が受信する。
    /// 1 user 1 receiver (mpsc) 構造で、複数 actor が 1 通知を奪い合う事故を回避。
    notification_routes: Mutex<HashMap<String, mpsc::Sender<String>>>,
    /// load phase の join 成功カウンタ。ramp_controller の階段 trigger に使う。
    /// pretest / integrity の join は積まない (= load 内で actor 呼出し側からのみ加算)。
    /// 名前で「load 限定」を明示し、helper や validation 側からの誤用を防ぐ。
    load_join_success_count: AtomicU64,
    /// load phase 中の close 初観測を seller actor に流す bounded channel の sender。
    /// run_load_phase 開始時に install / 終了時に take() することで、
    /// pretest / integrity 期の close は流れない (Phase::Load ガードと二重防御)。
    /// `notify_load_close` は `try_send` で fill 時 drop (= seller_queue_dropped++)、
    /// 「観測された close 数」ではなく「bench 内 queue 消化能力」が負荷形状を決め始めるのを防ぐ。
    load_close_tx: Mutex<Option<mpsc::Sender<()>>>,
    /// seller actor に enqueue できた close event 数 (= try_send 成功)。
    seller_events_emitted: AtomicU64,
    /// seller queue fill (`TrySendError::Full`) で drop された close event 数。queue 容量
    /// キャリブレーション指標。`Closed` は受信側喪失なので `seller_channel_closed` で別計上。
    seller_queue_dropped: AtomicU64,
    /// seller receiver 喪失 (`TrySendError::Closed`、create_user 失敗等) 後の close event 数。
    /// "seller が動いていない" の指標で、queue 容量問題とは切り分ける。
    seller_channel_closed: AtomicU64,
    /// seller actor が成功した create_campaign の数。
    seller_campaigns_created: AtomicU64,
    /// seller actor の create_campaign で発生したエラー件数。
    seller_create_errors: AtomicU64,
}

impl BenchState {
    fn new(fixtures: Vec<ImageFixture>, rng_seed: u64) -> Self {
        Self {
            notifications: Mutex::new(HashSet::new()),
            observed_max_count: Mutex::new(HashMap::new()),
            closed_campaigns: Mutex::new(HashMap::new()),
            score_total: AtomicI64::new(0),
            critical: Mutex::new(Vec::new()),
            soft_count: AtomicU64::new(0),
            cancel_global: CancellationToken::new(),
            fixtures,
            posted_campaigns: Mutex::new(HashMap::new()),
            rng: Mutex::new(StdRng::seed_from_u64(rng_seed)),
            notification_routes: Mutex::new(HashMap::new()),
            load_join_success_count: AtomicU64::new(0),
            load_close_tx: Mutex::new(None),
            seller_events_emitted: AtomicU64::new(0),
            seller_queue_dropped: AtomicU64::new(0),
            seller_channel_closed: AtomicU64::new(0),
            seller_campaigns_created: AtomicU64::new(0),
            seller_create_errors: AtomicU64::new(0),
        }
    }

    /// seller actor 向け close event channel を install する (run_load_phase 用)。
    fn install_load_close_channel(&self, tx: mpsc::Sender<()>) {
        *self.load_close_tx.lock().unwrap() = Some(tx);
    }

    /// load phase 終了時に sender を drop し、seller actor の receiver を閉じる。
    fn close_load_close_channel(&self) {
        self.load_close_tx.lock().unwrap().take();
    }

    /// load phase 中の close 初観測を seller queue に push する。
    /// channel 未 install / queue 満杯 / receiver drop 後は drop してカウンタだけ進める。
    /// `closed_campaigns` の lock を抜けてから呼ぶこと (lock-cross drop 回避)。
    ///
    /// drop 経路は 3 つ別カウンタに分類する:
    /// - 未 install: 何もしない (load phase 開始前 / 終了後)
    /// - `TrySendError::Full`: bounded 1024 backlog 飽和 → `seller_queue_dropped`
    /// - `TrySendError::Closed`: seller actor が早期 return (create_user 失敗等) → `seller_channel_closed`
    /// queue 容量のキャリブレーション指標として `Full` と `Closed` を混ぜない。
    fn notify_load_close(&self) {
        use tokio::sync::mpsc::error::TrySendError;
        let tx = self.load_close_tx.lock().unwrap().clone();
        let Some(tx) = tx else { return };
        match tx.try_send(()) {
            Ok(()) => {
                self.seller_events_emitted.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Full(())) => {
                self.seller_queue_dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Closed(())) => {
                self.seller_channel_closed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// load phase 内の join 成功 (= 200 OK) を 1 件積み、加算後の総数を返す。
    /// ramp_controller の階段 trigger (= count >= next_threshold) に使う。
    /// helper (`join_after_me_check`) ではなく load actor の呼出し側で発火させ、
    /// pretest / integrity の join を ramp 起動前に消費させない。
    fn record_load_join_success(&self) -> u64 {
        self.load_join_success_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn pick_fixture(&self) -> ImageFixture {
        let mut rng = self.rng.lock().unwrap();
        let idx = rng.gen_range(0..self.fixtures.len());
        self.fixtures[idx].clone()
    }

    fn register_posted(&self, campaign_id: &str, expected_hash: &str) {
        self.posted_campaigns
            .lock()
            .unwrap()
            .insert(campaign_id.to_string(), expected_hash.to_string());
    }

    /// bench 自身が POST した campaign の期待 image_hash を返す。
    /// seed campaign の hash 比較は false positive 回避のため行わない。
    fn expected_posted_hash(&self, campaign_id: &str) -> Option<String> {
        self.posted_campaigns
            .lock()
            .unwrap()
            .get(campaign_id)
            .cloned()
    }

    /// soft error を 1 件計上する。fetch_add の戻り値 + 1 が「この add 直後の総数」となり、
    /// 各 task で一意なので、ちょうど SOFT_FAIL_THRESHOLD を観測した 1 task だけが
    /// cancel_global.cancel() を呼ぶ (= 多重発火しない)。CancellationToken::cancel() 自体も
    /// 冪等なので、threshold の二重観測が起きても安全側に倒れる。Ordering::Relaxed は
    /// カウンタ単独の atomic 更新で十分 (cancel との因果は CancellationToken 内部で確立)。
    fn add_soft(&self, msg: impl Into<String>) -> u64 {
        let count = self.soft_count.fetch_add(1, Ordering::Relaxed) + 1;
        eprintln!("[SOFT] {}", msg.into());
        if count == SOFT_FAIL_THRESHOLD {
            eprintln!("[SOFT] threshold reached ({SOFT_FAIL_THRESHOLD}); cancelling benchmark");
            self.cancel_global.cancel();
        }
        count
    }

    fn add_critical(&self, msg: impl Into<String>) {
        let m = msg.into();
        eprintln!("[CRITICAL] {m}");
        self.critical.lock().unwrap().push(m);
    }

    /// webhook 受信記録。新規挿入できれば true、重複なら critical 化して false を返す。
    /// 戻り値で重複を分岐し、新規時のみ user 別 channel に dispatch するために使う。
    fn record_webhook(&self, user_id: &str, campaign_id: &str) -> bool {
        let key = (user_id.to_string(), campaign_id.to_string());
        let inserted = self.notifications.lock().unwrap().insert(key);
        if !inserted {
            self.add_critical(format!(
                "duplicate webhook: user={user_id} campaign={campaign_id}"
            ));
        }
        inserted
    }

    /// campaign snapshot に対する API 局所不変条件 + critical 検査を一括で行う。
    /// idea.md: "participants.length == current_count" / "closed なら current == goal == participants"。
    /// 違反があれば critical を積んで早期 return し、closed の score 加算には進めない。
    ///
    /// 局所不変 (status 由来の自己矛盾) は stale 判定より先に検査する: 単発レスポンスの内部矛盾は
    /// 並列観測のレースとは独立で、stale でも仕様違反として critical 化すべきだから。
    /// 観測順序ベースの一貫性 (closed→open 状態逆転等) は stale snap で false positive になるため、
    /// `current_count < observed_max` (= 並列下のレース由来) は status 比較の入力にしない。
    fn record_campaign_observation(&self, c: &CampaignSnap, phase: Phase) {
        // (1) participants.len == current_count (idea.md API 仕様)。stale 含めて常に成立すべき。
        if c.participants.len() as i32 != c.current_count {
            self.add_critical(format!(
                "participants/current_count mismatch: campaign={} participants={} current={}",
                c.id,
                c.participants.len(),
                c.current_count
            ));
            return;
        }
        // (2) goal_count overflow (= idea.md critical 一覧)。stale でも仕様上 0 件期待。
        if c.current_count > c.goal_count {
            self.add_critical(format!(
                "goal_count overflow: campaign={} current={} goal={}",
                c.id, c.current_count, c.goal_count
            ));
            return;
        }
        // (3) status の値域。spec で "open" / "closed" のみ。stale 判定の前に弾く。
        if c.status != "open" && c.status != "closed" {
            self.add_critical(format!(
                "invalid campaign status: campaign={} status={:?}",
                c.id, c.status
            ));
            return;
        }
        // (4) status=closed なら current == goal == participants (idea.md API 仕様)。
        //     stale でも自己矛盾なら critical (単発レスポンス内の派生計算バグの捕捉)。
        if c.status == "closed"
            && (c.current_count != c.goal_count
                || c.participants.len() as i32 != c.goal_count)
        {
            self.add_critical(format!(
                "closed invariant violation: campaign={} current={} goal={} participants={}",
                c.id,
                c.current_count,
                c.goal_count,
                c.participants.len()
            ));
            return;
        }
        // (5) status=open で current_count >= goal_count は仕様違反 (= 状態逆転 / 派生計算バグ検出)。
        //     bench 観測順に依存せず、webapp 単発レスポンスの内部矛盾として critical 化。
        if c.status == "open" && c.current_count >= c.goal_count {
            self.add_critical(format!(
                "open with current_count >= goal_count: campaign={} current={} goal={}",
                c.id, c.current_count, c.goal_count
            ));
            return;
        }

        // monotonicity check: current_count は webapp で participants 行追加のみで増える単調非減少値。
        // bench 観測値が過去の最大値より小さいなら、並列下で受け取った stale snapshot。
        // stale は score 加算 / closed 登録の対象から外す (重複登録回避)。
        let stale = {
            let mut max_counts = self.observed_max_count.lock().unwrap();
            let prev = max_counts.get(&c.id).copied().unwrap_or(0);
            if c.current_count < prev {
                true
            } else {
                max_counts.insert(c.id.clone(), c.current_count);
                false
            }
        };
        if stale {
            return;
        }

        // (6) closed 初観測 → finalcheck 用 record + Load 期なら score 加点 (= idea.md スコア式)。
        //     Validation 期 (integrity_scenario 等) の close は charges 突合に含めるが score には入れない。
        //     `seller actor` 向け close event は Load 期初観測のみ enqueue する (= 1 close 1 event)。
        //     channel send は closed_campaigns lock を抜けてから (lock-cross 回避)。
        if c.status == "closed" {
            let mut emit_load_close = false;
            {
                let mut closed = self.closed_campaigns.lock().unwrap();
                if !closed.contains_key(&c.id) {
                    let participants: Vec<String> =
                        c.participants.iter().map(|p| p.user_id.clone()).collect();
                    let n = participants.len() as i64;
                    if phase == Phase::Load {
                        self.score_total.fetch_add(n * 1000, Ordering::Relaxed);
                        emit_load_close = true;
                    }
                    closed.insert(c.id.clone(), ClosedRecord { participants });
                }
            }
            if emit_load_close {
                self.notify_load_close();
            }
        }
    }
}

// === bench error / reqwest 分類 ===

/// HTTP 経路のエラー分類。`Soft` は docs/manual.md §6.5 で soft 計上対象になる事象
/// (per-req 10s timeout / レスポンス形式不正 / 想定外 status コード) で、生成時点で
/// `add_soft` 済 (= 二重計上禁止)。`Other` は transport 失敗 / 不変条件違反 / scenario
/// 内部の invariant エラーなど soft 計上対象外で、main の match で `add_critical` 化する。
///
/// run_scenario は両方を `?` で伝播させ、main の match で挙動を分岐する:
///   - Soft : critical 化せず eprintln のみ (soft 計上は完了している)
///   - Other: add_critical 化 (= scenario abort = score 0)
#[derive(Debug)]
enum BenchError {
    Soft(String),
    Other(String),
}

impl std::fmt::Display for BenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchError::Soft(ctx) => write!(f, "soft-counted error: {ctx}"),
            BenchError::Other(s) => write!(f, "{s}"),
        }
    }
}

type BenchResult<T> = Result<T, BenchError>;

/// `reqwest::Error` を分類して BenchError に変換する。
/// `is_timeout()` は `.send()` / `.json()` / `.bytes()` のいずれの段階でも、`Client::timeout`
/// 超過で true になるので、reqwest::Error が出る全箇所 (送信 / body read) でこの関数を通す。
/// `is_decode()` はレスポンスの JSON パース失敗 (= manual.md §6.5「レスポンス形式不正」)。
fn map_reqwest(state: &BenchState, ctx: &str, e: reqwest::Error) -> BenchError {
    if e.is_timeout() {
        state.add_soft(format!("{ctx}: timeout (per-req 10s)"));
        BenchError::Soft(ctx.to_string())
    } else if e.is_decode() {
        state.add_soft(format!("{ctx}: response decode error: {e}"));
        BenchError::Soft(ctx.to_string())
    } else {
        BenchError::Other(format!("{ctx}: {e}"))
    }
}

/// `error_for_status()` 失敗 (= 想定外 status コード) を soft 計上 + Soft 化するヘルパー。
/// manual.md §6.5「ステータスコード不一致 = soft」に合わせる。
/// 後続処理の前提が崩れる (= その helper 自身は続行不能) ため scenario abort 経路は維持し、
/// main の match で critical 化しないよう Soft で返す。
fn map_status(state: &BenchState, ctx: &str, e: reqwest::Error) -> BenchError {
    state.add_soft(format!("{ctx}: unexpected status: {e}"));
    BenchError::Soft(ctx.to_string())
}

// === DTOs (webapp レスポンスをパースする) ===

#[derive(Deserialize, Debug, Clone)]
struct CampaignSnap {
    id: String,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    #[serde(default)]
    description: String,
    #[allow(dead_code)]
    price: i32,
    goal_count: i32,
    current_count: i32,
    #[allow(dead_code)]
    #[serde(default)]
    tags: Vec<String>,
    status: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    last_joined_at: Option<String>,
    #[serde(default)]
    participants: Vec<ParticipantSnap>,
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct ParticipantSnap {
    user_id: String,
    name: String,
    joined_at: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ChargeSnap {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    amount: i32,
    campaign: ChargeCampaignSnap,
    #[allow(dead_code)]
    created_at: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ChargeCampaignSnap {
    id: String,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    price: i32,
}

#[derive(Deserialize, Debug, Clone)]
struct MeSnap {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    credit_limit: i32,
    credit_used: i32,
}

#[derive(Debug, Clone)]
struct UserToken {
    id: String,
    #[allow(dead_code)]
    name: String,
}

// === main ===

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let fd: i32 = env::var("REPORT_FD")
        .or_else(|_| env::var("ISUXBENCH_REPORT_FD"))
        .ok()
        .and_then(|s| s.parse().ok())
        .expect("REPORT_FD or ISUXBENCH_REPORT_FD must be set to a valid fd number");

    let target_ip = env::var("BENCHWARMER_target_ip").unwrap_or_else(|_| "127.0.0.1".to_string());
    // webapp は API を `/api/` 配下にマウントしている (docs/idea.md)。base に prefix まで含めて
    // おくことで、各 API call は `format!("{base}/initialize")` のように仕様 path をそのまま
    // 連結すれば良い。`/healthz` と bench 自身の receiver `webhook_url` は別経路。
    //
    // base URL は WEBAPP_BASE_URL を最優先で受け、未設定なら BENCHWARMER_target_ip と
    // WEBAPP_PORT (default 8080、production AMI は bench.sh で 80 を渡す) で組む。
    let webapp_base_url = env::var("WEBAPP_BASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            let port: u16 = env::var("WEBAPP_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .expect("WEBAPP_PORT must be a valid u16");
            format!("http://{target_ip}:{port}")
        });
    let target_base = format!("{}/api", webapp_base_url.trim_end_matches('/'));
    let webhook_url =
        env::var("WEBHOOK_URL").unwrap_or_else(|_| format!("http://{target_ip}:9999/webhook"));

    // fixture ディレクトリスキャン (起動時 1 回、SHA256 を事前計算)。
    // dev fast cycle 用に BENCH_FIXTURES_LIMIT で件数を絞れる。
    let fixtures_dir =
        env::var("BENCH_FIXTURES_DIR").unwrap_or_else(|_| "/opt/bench/fixtures/images".to_string());
    let fixtures_limit = env::var("BENCH_FIXTURES_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok());
    let fixtures = match load_fixtures(&fixtures_dir, fixtures_limit) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ERROR: bench fixture load: {e}");
            // 配備ミスは critical detection ではなく運用エラーとして score=0 で終了
            let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
            writeln!(f, r#"{{"score":0}}"#).expect("write report");
            return;
        }
    };
    eprintln!(
        "bench: loaded {} fixtures from {}",
        fixtures.len(),
        fixtures_dir
    );

    let rng_seed: u64 = env::var("BENCH_RNG_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0xC0FF_EE00);
    let state = Arc::new(BenchState::new(fixtures, rng_seed));

    // webhook サーバ起動 (0.0.0.0:9999)
    let app = Router::new()
        .route("/webhook", post(webhook_handler))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:9999")
        .await
        .expect("bind 0.0.0.0:9999 (webhook receiver)");
    let server_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("webhook server: {e}");
        }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 通常リクエストは per-req 10s タイムアウト (docs/manual.md §6.3 / §6.5)。超過は
    // soft error 1 件として計上する。/api/initialize のみ INITIALIZE_TIMEOUT (30s) を
    // per-request override で適用 (run_scenario 参照)。
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("reqwest client");

    match run_scenario(&client, &target_base, &webhook_url, state.clone()).await {
        Ok(()) => {}
        Err(BenchError::Soft(ctx)) => {
            // map_reqwest / map_status / その他 soft 経路で既に add_soft 済 (= 二重計上禁止)。
            // critical には積まず eprintln のみ。結果は soft>=1 として score 計算に算入される。
            eprintln!("scenario aborted (already counted as soft): {ctx}");
        }
        Err(BenchError::Other(e)) => {
            state.add_critical(format!("scenario aborted: {e}"));
        }
    }

    let critical_count = state.critical.lock().unwrap().len();
    let soft_count = state.soft_count.load(Ordering::Relaxed);
    let total = state.score_total.load(Ordering::Relaxed);
    let score = compute_score(critical_count, soft_count, total);
    if critical_count == 0 && total == 0 && soft_count == 0 {
        // load phase が close を 1 件も生成できていない = 競技として壊れている。
        // 本番では「actor 並列度 / load_duration / candidate 選択戦略の見直し」のシグナル。
        eprintln!(
            "[WARN] score_total == 0 with critical 0 / soft 0: load phase produced no closed campaigns"
        );
    }
    eprintln!(
        "bench: critical={critical_count} soft={soft_count} score_total={total} score={score}"
    );
    eprintln!(
        "seller: events_emitted={} queue_dropped={} channel_closed={} campaigns_created={} create_errors={}",
        state.seller_events_emitted.load(Ordering::Relaxed),
        state.seller_queue_dropped.load(Ordering::Relaxed),
        state.seller_channel_closed.load(Ordering::Relaxed),
        state.seller_campaigns_created.load(Ordering::Relaxed),
        state.seller_create_errors.load(Ordering::Relaxed),
    );

    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    writeln!(f, r#"{{"score":{score}}}"#).expect("write report to REPORT_FD");
    drop(f);

    server_handle.abort();
}

// === webhook handler ===

async fn webhook_handler(State(state): State<Arc<BenchState>>, body: Bytes) -> StatusCode {
    // Json extractor だと Content-Type / 形式不正 を勝手に 4xx に倒してしまうため、
    // 生 bytes で受けて自前 parse する。critical 検知を漏らさないため。
    let body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            state.add_critical(format!("malformed webhook json: {e}"));
            return StatusCode::CREATED;
        }
    };
    let typ = body.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if typ != "campaign_closing_soon" {
        state.add_critical(format!("unexpected webhook type: {typ}"));
        return StatusCode::CREATED;
    }
    let user_id = body.get("user_id").and_then(|v| v.as_str()).unwrap_or("");
    let campaign_id = body
        .get("campaign")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if user_id.is_empty() || campaign_id.is_empty() {
        state.add_critical(format!("malformed webhook body: {body}"));
        return StatusCode::CREATED;
    }
    let inserted = state.record_webhook(user_id, campaign_id);
    if inserted {
        // notification_actor の channel に dispatch (登録済の user のみ)。
        // try_send で fill 時は drop。仕様上 "通知は送信されなくてもいい / 遅延 OK" なので
        // drop しても critical にはしない。lock は短く持ちたいので Sender だけ clone して抜ける。
        let tx = state
            .notification_routes
            .lock()
            .unwrap()
            .get(user_id)
            .cloned();
        if let Some(tx) = tx {
            let _ = tx.try_send(campaign_id.to_string());
        }
    }
    StatusCode::CREATED
}

// === scenario ===

async fn run_scenario(
    client: &Client,
    base: &str,
    webhook_url: &str,
    state: Arc<BenchState>,
) -> BenchResult<()> {
    eprintln!("==> initialize");
    // /api/initialize は docs/manual.md §6.3 で 30s 制約。per-req override で別枠扱い。
    // initialize の timeout は soft 1 件ではなく即時 FAIL なので map_reqwest を通さず Other 化。
    let resp = client
        .post(format!("{base}/initialize"))
        .timeout(INITIALIZE_TIMEOUT)
        .json(&json!({ "notification_webhook_url": webhook_url }))
        .send()
        .await
        .map_err(|e| BenchError::Other(format!("initialize send: {e}")))?;
    if !resp.status().is_success() {
        return Err(BenchError::Other(format!(
            "initialize status {}",
            resp.status()
        )));
    }

    eprintln!("==> pretest");
    pretest(client, base, &state).await?;

    eprintln!("==> negative probes");
    negative_probes(client, base, &state).await?;

    eprintln!("==> integrity scenario");
    integrity_scenario(client, base, &state).await?;

    eprintln!("==> load phase");
    run_load_phase(client, base, state.clone()).await?;

    eprintln!("==> finalcheck");
    finalcheck(client, base, &state).await?;

    Ok(())
}

/// 異常系の status code を観測し、仕様外なら critical を積む。
async fn negative_probes(client: &Client, base: &str, state: &BenchState) -> BenchResult<()> {
    // 認証無しで /api/campaigns → 401
    let resp = client
        .get(format!("{base}/campaigns"))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "noauth send", e))?;
    if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
        state.add_critical(format!(
            "GET /api/campaigns without X-User-ID expected 401 got {}",
            resp.status()
        ));
    }

    // 未知 user で /api/campaigns → 401
    let resp = client
        .get(format!("{base}/campaigns"))
        .header("X-User-ID", "00000000-0000-4000-8000-deadbeefdead")
        .send()
        .await
        .map_err(|e| map_reqwest(state, "unknown user send", e))?;
    if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
        state.add_critical(format!(
            "GET /api/campaigns with unknown user expected 401 got {}",
            resp.status()
        ));
    }

    let admin = create_user(client, base, state, "negative-admin").await?;

    // 不在 campaign id → 404
    let resp = client
        .get(format!(
            "{base}/campaigns/00000000-0000-0000-0000-000000000000"
        ))
        .header("X-User-ID", &admin.id)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "nonexistent campaign send", e))?;
    if resp.status() != reqwest::StatusCode::NOT_FOUND {
        state.add_critical(format!(
            "GET /api/campaigns/{{nonexistent}} expected 404 got {}",
            resp.status()
        ));
    }

    // saved_searches 重複 tag → 400
    let resp = client
        .post(format!("{base}/saved_searches"))
        .header("X-User-ID", &admin.id)
        .json(&json!({ "tags": [seed_data::tag::MESH, seed_data::tag::MESH] }))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "dup tag send", e))?;
    if resp.status() != reqwest::StatusCode::BAD_REQUEST {
        state.add_critical(format!(
            "POST /api/saved_searches dup tags expected 400 got {}",
            resp.status()
        ));
    }

    // saved_searches 未知 tag → 400
    let resp = client
        .post(format!("{base}/saved_searches"))
        .header("X-User-ID", &admin.id)
        .json(&json!({ "tags": ["zzznonexistent"] }))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "unknown tag send", e))?;
    if resp.status() != reqwest::StatusCode::BAD_REQUEST {
        state.add_critical(format!(
            "POST /api/saved_searches unknown tag expected 400 got {}",
            resp.status()
        ));
    }

    // ===== image 関連の異常系 =====
    // すべて soft (= 4xx ハズレは plan で減点扱い)。

    // POST /api/campaigns: image フィールド欠落 → 400
    let body = json!({
        "name": "no-image", "description": "x", "price": 2000, "goal_count": 2,
        "tags": [seed_data::tag::MESH]
    });
    let resp = client
        .post(format!("{base}/campaigns"))
        .header("X-User-ID", &admin.id)
        .json(&body)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "missing-image send", e))?;
    if resp.status() != reqwest::StatusCode::BAD_REQUEST {
        state.add_soft(format!(
            "POST /api/campaigns without image expected 400 got {}",
            resp.status()
        ));
    }

    // POST /api/campaigns: base64 不正 → 400
    let body = json!({
        "name": "bad-b64", "description": "x", "price": 2000, "goal_count": 2,
        "tags": [seed_data::tag::MESH], "image": "!!!not-base64!!!"
    });
    let resp = client
        .post(format!("{base}/campaigns"))
        .header("X-User-ID", &admin.id)
        .json(&body)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "bad-b64 send", e))?;
    if resp.status() != reqwest::StatusCode::BAD_REQUEST {
        state.add_soft(format!(
            "POST /api/campaigns with bad base64 expected 400 got {}",
            resp.status()
        ));
    }

    // POST /api/campaigns: PNG magic → 400
    let png_b64 = b64_encode(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let body = json!({
        "name": "png-mag", "description": "x", "price": 2000, "goal_count": 2,
        "tags": [seed_data::tag::MESH], "image": png_b64
    });
    let resp = client
        .post(format!("{base}/campaigns"))
        .header("X-User-ID", &admin.id)
        .json(&body)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "png-magic send", e))?;
    if resp.status() != reqwest::StatusCode::BAD_REQUEST {
        state.add_soft(format!(
            "POST /api/campaigns with PNG magic expected 400 got {}",
            resp.status()
        ));
    }

    // POST /api/campaigns: 200 KiB 超 → 413
    let mut big = vec![0u8; 204_801];
    big[0] = 0xFF;
    big[1] = 0xD8;
    big[2] = 0xFF;
    let big_b64 = b64_encode(&big);
    let body = json!({
        "name": "oversize", "description": "x", "price": 2000, "goal_count": 2,
        "tags": [seed_data::tag::MESH], "image": big_b64
    });
    let resp = client
        .post(format!("{base}/campaigns"))
        .header("X-User-ID", &admin.id)
        .json(&body)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "oversize send", e))?;
    if resp.status() != reqwest::StatusCode::PAYLOAD_TOO_LARGE {
        state.add_soft(format!(
            "POST /api/campaigns with >200 KiB expected 413 got {}",
            resp.status()
        ));
    }

    // GET /api/campaigns/{seed_id}/image without auth → 401
    // seed-data に固定された 5 件の base campaign のうち先頭を使う。
    let seed_cid = seed_data::BASE_CAMPAIGNS[0].id;
    let resp = client
        .get(format!("{base}/campaigns/{seed_cid}/image"))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "noauth image send", e))?;
    if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
        state.add_soft(format!(
            "GET /api/campaigns/{{seed}}/image without X-User-ID expected 401 got {}",
            resp.status()
        ));
    }

    // GET /api/campaigns/{nonexistent}/image → 404
    let resp = client
        .get(format!(
            "{base}/campaigns/00000000-0000-0000-0000-000000000000/image"
        ))
        .header("X-User-ID", &admin.id)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "nonexistent image send", e))?;
    if resp.status() != reqwest::StatusCode::NOT_FOUND {
        state.add_soft(format!(
            "GET /api/campaigns/{{nonexistent}}/image expected 404 got {}",
            resp.status()
        ));
    }

    // ===== credit insufficient (402) =====
    //
    // price=20000 / goal_count=2 の campaign を 4 個作って、別 user が順に join。
    // creator は auto-join しない仕様 (idea.md) なので、他者の join がないあいだ各 campaign は
    // current_count=1 のまま open を維持し credit_used が累積する:
    //   join1: credit_used = 20000 ≤ 60000 → 200
    //   join2: credit_used = 40000 ≤ 60000 → 200
    //   join3: credit_used = 60000 ≤ 60000 → 200 (= limit ぴったり、boundary 確認)
    //   join4: credit_used = 80000 > 60000 → 402 期待 (pre-check 発火)
    let credit_user = create_user(client, base, state, "credit-prober").await?;
    let mut probe_campaign_ids: Vec<String> = Vec::new();
    for i in 0..4 {
        let cresp = create_campaign(
            client,
            base,
            state,
            &admin,
            &json!({
                "name": format!("credit-probe-{i}"),
                "description": "credit-probe campaign",
                "price": 20000,
                "goal_count": 2,
                "tags": [seed_data::tag::MESH]
            }),
        )
        .await?;
        probe_campaign_ids.push(cresp.id);
    }
    for (i, pid) in probe_campaign_ids.iter().take(3).enumerate() {
        let resp = client
            .post(format!("{base}/campaigns/{pid}/join"))
            .header("X-User-ID", &credit_user.id)
            .json(&json!({}))
            .send()
            .await
            .map_err(|e| map_reqwest(state, &format!("credit-probe join#{i} send"), e))?;
        if resp.status() != reqwest::StatusCode::OK {
            state.add_critical(format!(
                "credit-probe join#{i} (cumulative credit_used={}) expected 200 got {}",
                (i + 1) * 20000,
                resp.status()
            ));
            continue;
        }
        let snap: CampaignSnap = resp
            .json()
            .await
            .map_err(|e| map_reqwest(state, &format!("credit-probe join#{i} json"), e))?;
        state.record_campaign_observation(&snap, Phase::Validation);
        if snap.current_count != 1 || snap.status != "open" {
            state.add_critical(format!(
                "credit-probe join#{i} should leave campaign open/1 got {}/{}",
                snap.status, snap.current_count
            ));
        }
    }
    // boundary: 3 join 後の credit_used が credit_limit ぴったり (60000) であることを確認。
    // ここで credit_used != 60000 なら、4 回目の 402 期待は前提が崩れているので probe 全体無効。
    let me_after_3 = get_me(
        client,
        base,
        &credit_user,
        state,
        "credit-probe after 3 joins (boundary)",
    )
    .await?;
    if me_after_3.credit_used != 60000 {
        state.add_critical(format!(
            "credit-probe credit_used after 3 joins: expected 60000 (boundary) got {}",
            me_after_3.credit_used
        ));
    }
    // 4 つ目: 与信不足 → 402 期待
    let pid = &probe_campaign_ids[3];
    let resp = client
        .post(format!("{base}/campaigns/{pid}/join"))
        .header("X-User-ID", &credit_user.id)
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "credit-probe 4th join send", e))?;
    if resp.status() != reqwest::StatusCode::PAYMENT_REQUIRED {
        state.add_critical(format!(
            "credit-probe 4th join (would exceed credit_limit) expected 402 got {}",
            resp.status()
        ));
    }

    Ok(())
}

async fn pretest(client: &Client, base: &str, state: &BenchState) -> BenchResult<()> {
    // GET /api/tags
    let tags: Vec<String> = client
        .get(format!("{base}/tags"))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "tags send", e))?
        .error_for_status()
        .map_err(|e| map_status(state, "tags status", e))?
        .json()
        .await
        .map_err(|e| map_reqwest(state, "tags json", e))?;
    if tags.len() != seed_data::TAGS.len() {
        return Err(BenchError::Other(format!(
            "tags expected {} got {}",
            seed_data::TAGS.len(),
            tags.len()
        )));
    }
    // bench は seed 由来の必須 tag (REQUIRED_TAG_NAMES = mesh / ergonomic) に
    // 依存するため、seed 破損による diagnostics を明示する (= create_campaign 400
    // にすり替わるのを回避)。
    for required in seed_data::REQUIRED_TAG_NAMES.iter().copied() {
        if !tags.iter().any(|t| t == required) {
            return Err(BenchError::Other(format!(
                "seed-required tag {:?} missing from /api/tags response: {:?}",
                required, tags
            )));
        }
    }

    // POST /api/users (no auth)
    let admin = create_user(client, base, state, "pretest-admin").await?;

    // GET /api/me: 仕様 (docs/idea.md) で fresh user は credit_used=0、credit_limit=60000 (固定値)。
    // 形状検査として exact 値で確認 (将来 credit_limit を変える場合はここを更新)。
    let me = get_me(client, base, &admin, state, "pretest fresh admin").await?;
    if me.credit_limit != 60000 {
        return Err(BenchError::Other(format!(
            "fresh user credit_limit expected 60000 got {}",
            me.credit_limit
        )));
    }
    if me.credit_used != 0 {
        return Err(BenchError::Other(format!(
            "fresh user credit_used expected 0 got {}",
            me.credit_used
        )));
    }

    // GET /api/campaigns (auth required)
    let campaigns = list_campaigns(client, base, state, &admin, "", "new").await?;
    assert_status_open_only(state, &campaigns, "pretest sort=new");
    assert_sort_order(state, &campaigns, "new", "pretest sort=new");
    for c in &campaigns {
        state.record_campaign_observation(c, Phase::Validation);
    }

    // GET /api/campaigns/{seed_id} (seed-data の先頭 base campaign)
    let seed_cid = seed_data::BASE_CAMPAIGNS[0].id.to_string();
    let c = get_campaign(client, base, state, &admin, &seed_cid).await?;
    state.record_campaign_observation(&c, Phase::Validation);

    // POST /api/campaigns
    let cresp = create_campaign(
        client,
        base,
        state,
        &admin,
        &json!({
            "name": "pretest campaign",
            "description": "pretest desc",
            "price": 2000,
            "goal_count": 3,
            "tags": [seed_data::tag::MESH]
        }),
    )
    .await?;
    if cresp.current_count != 0 || cresp.status != "open" {
        return Err(BenchError::Other(format!(
            "freshly created campaign should have current_count=0 status=open, got {}/{}",
            cresp.current_count, cresp.status
        )));
    }
    state.record_campaign_observation(&cresp, Phase::Validation);

    // POST /api/saved_searches
    create_saved_search(client, base, state, &admin, &[seed_data::tag::MESH]).await?;

    // GET /api/charges (empty)
    let charges = list_charges(client, base, state, &admin).await?;
    if !charges.is_empty() {
        return Err(BenchError::Other(format!(
            "admin charges should be empty got {}",
            charges.len()
        )));
    }

    // sort=active path も最低限叩いておく。
    // 新仕様: 0 参加 campaign も含み、ソートキーは COALESCE(last_joined_at, created_at) DESC。
    let active = list_campaigns(client, base, state, &admin, "", "active").await?;
    assert_status_open_only(state, &active, "pretest sort=active");
    assert_sort_order(state, &active, "active", "pretest sort=active");
    // 0 参加除外が「無いこと」を直接焼く: 直前に作成した pretest cresp は created_at 最新の
    // 0 参加 campaign。新仕様なら active list 上位 30 件に必ず含まれ、current_count=0 /
    // last_joined_at=null のはず。旧実装のまま 0 参加除外を残すと None となり critical 化する。
    match active.iter().find(|c| c.id == cresp.id) {
        Some(c) if c.current_count == 0 && c.last_joined_at.is_none() => {}
        Some(c) => state.add_critical(format!(
            "pretest sort=active: freshly created 0-participant campaign has invalid count/last_joined_at: id={} current_count={} last_joined_at={:?}",
            c.id, c.current_count, c.last_joined_at
        )),
        None => state.add_critical(format!(
            "pretest sort=active: freshly created 0-participant campaign missing from active list (0 参加除外が残っている?): id={}",
            cresp.id
        )),
    }
    for c in &active {
        state.record_campaign_observation(c, Phase::Validation);
    }

    Ok(())
}

async fn integrity_scenario(
    client: &Client,
    base: &str,
    state: &BenchState,
) -> BenchResult<()> {
    let creator = create_user(client, base, state, "scenario-creator").await?;
    let user_a = create_user(client, base, state, "scenario-a").await?;
    let user_b = create_user(client, base, state, "scenario-b").await?;
    let user_c = create_user(client, base, state, "scenario-c").await?;

    // user_a の saved_search で tag "ergonomic" を保存
    create_saved_search(client, base, state, &user_a, &[seed_data::tag::ERGONOMIC]).await?;

    // creator が goal_count=2、tag=ergonomic で campaign を作成
    let cresp = create_campaign(
        client,
        base,
        state,
        &creator,
        &json!({
            "name": "scenario campaign",
            "description": "scenario desc",
            "price": 2000,
            "goal_count": 2,
            "tags": [seed_data::tag::ERGONOMIC]
        }),
    )
    .await?;
    let cid = cresp.id.clone();
    state.record_campaign_observation(&cresp, Phase::Validation);

    // user_b が join: (b) check 経由 (= /api/me で残高十分を確認した直後の 402 を critical 化)
    // (after = goal_count - 1 = 1 → user_a 宛に webhook 期待)
    let after_b = match join_after_me_check(
        client,
        base,
        &user_b,
        state,
        &cresp,
        "user B before join",
    )
    .await?
    {
        JoinChecked::Joined(snap) => snap,
        JoinChecked::Conflict => {
            return Err(BenchError::Other(
                "user B join unexpectedly returned 409 (this scenario is sequential)".into(),
            ));
        }
    };
    if after_b.current_count != 1 {
        return Err(BenchError::Other(format!(
            "after B current_count expected 1 got {}",
            after_b.current_count
        )));
    }
    state.record_campaign_observation(&after_b, Phase::Validation);

    // user_b の credit_used: campaign は open (current_count=1 < goal_count=2) → price=2000 が乗る
    let me_b_open = get_me(client, base, &user_b, state, "after B joined open").await?;
    if me_b_open.credit_used != 2000 {
        state.add_critical(format!(
            "user B credit_used after join (open): expected 2000 got {}",
            me_b_open.credit_used
        ));
    }

    // user_c が join: (b) check 経由 (after = goal_count = 2 → closed、charges 発生)
    let after_c = match join_after_me_check(
        client,
        base,
        &user_c,
        state,
        &cresp,
        "user C before join",
    )
    .await?
    {
        JoinChecked::Joined(snap) => snap,
        JoinChecked::Conflict => {
            return Err(BenchError::Other(
                "user C join unexpectedly returned 409 (this scenario is sequential)".into(),
            ));
        }
    };
    state.record_campaign_observation(&after_c, Phase::Validation);
    if after_c.status != "closed" || after_c.current_count != 2 {
        return Err(BenchError::Other(format!(
            "after C should be closed/2 got {}/{}",
            after_c.status, after_c.current_count
        )));
    }
    if after_c.participants.len() != 2 {
        return Err(BenchError::Other(format!(
            "after C participants expected 2 got {}",
            after_c.participants.len()
        )));
    }

    // close 後 (current_count == goal_count) は credit_used から該当 campaign が外れる仕様。
    // user_b / user_c のいずれもこの campaign 以外には参加していないので credit_used == 0。
    let me_b_closed = get_me(client, base, &user_b, state, "after C joined (closed)").await?;
    if me_b_closed.credit_used != 0 {
        state.add_critical(format!(
            "user B credit_used after close: expected 0 (refund) got {}",
            me_b_closed.credit_used
        ));
    }
    let me_c_closed = get_me(client, base, &user_c, state, "after C joined (closed)").await?;
    if me_c_closed.credit_used != 0 {
        state.add_critical(format!(
            "user C credit_used after close: expected 0 (no open campaign joined) got {}",
            me_c_closed.credit_used
        ));
    }

    // GET /api/campaigns/{C} → closed
    let c2 = get_campaign(client, base, state, &user_b, &cid).await?;
    state.record_campaign_observation(&c2, Phase::Validation);
    if c2.status != "closed" {
        return Err(BenchError::Other(format!(
            "GET /api/campaigns/{cid} expected closed got {}",
            c2.status
        )));
    }

    // 追加 join 試行: closed campaign に creator が join → 409 のはず
    // ここで 200 が返ってきたら goal_count 超過の懸念があるので
    // body もパースしておいて critical を踏ませる
    let extra = client
        .post(format!("{base}/campaigns/{cid}/join"))
        .header("X-User-ID", &creator.id)
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "post-closed join send", e))?;
    if extra.status() != reqwest::StatusCode::CONFLICT {
        state.add_critical(format!(
            "joining closed campaign expected 409 got {}",
            extra.status()
        ));
        if let Ok(body) = extra.json::<CampaignSnap>().await {
            state.record_campaign_observation(&body, Phase::Validation);
        }
    }

    // GET /api/campaigns?tags=ergonomic で C が含まれない (status=open のみ)
    let listed =
        list_campaigns(client, base, state, &user_b, seed_data::tag::ERGONOMIC, "new").await?;
    for c in &listed {
        state.record_campaign_observation(c, Phase::Validation);
    }
    if listed.iter().any(|c| c.id == cid) {
        return Err(BenchError::Other(
            "closed campaign appeared in /api/campaigns?tags=ergonomic".into(),
        ));
    }

    // GET /api/charges (B / C それぞれ)
    let charges_b = list_charges(client, base, state, &user_b).await?;
    let count_b: usize = charges_b.iter().filter(|ch| ch.campaign.id == cid).count();
    if count_b == 0 {
        state.add_critical(format!("missing charge for user B in campaign {cid}"));
    } else if count_b > 1 {
        state.add_critical(format!(
            "double charge for user B in campaign {cid}: {count_b} entries"
        ));
    }
    let charges_c = list_charges(client, base, state, &user_c).await?;
    let count_c: usize = charges_c.iter().filter(|ch| ch.campaign.id == cid).count();
    if count_c == 0 {
        state.add_critical(format!("missing charge for user C in campaign {cid}"));
    } else if count_c > 1 {
        state.add_critical(format!(
            "double charge for user C in campaign {cid}: {count_c} entries"
        ));
    }

    // creator は participant ではないので charges 0 件
    let charges_creator = list_charges(client, base, state, &creator).await?;
    if charges_creator.iter().any(|ch| ch.campaign.id == cid) {
        state.add_critical(format!(
            "creator unexpectedly charged for campaign {cid} (creator must not be auto-joined)"
        ));
    }

    // webhook の grace period (仕様で「遅延は許容」)
    tokio::time::sleep(Duration::from_secs(1)).await;

    // user_a への webhook が来たか (届かなかった場合は仕様上 critical ではない)
    let received = state
        .notifications
        .lock()
        .unwrap()
        .contains(&(user_a.id.clone(), cid.clone()));
    if !received {
        eprintln!("[INFO] webhook to user_a for {cid} not received within grace (allowed)");
    }

    // GET /api/campaigns/{cid}/image: bench 自前 campaign に対する画像配信検証
    image_integrity_checks(client, base, &user_b, &cid, state).await?;

    Ok(())
}

/// bench 自前 campaign に対して GET /api/campaigns/{id}/image の整合性を検証する。
///
/// - 200 + body bytes が POST 時の fixture と完全一致 (SHA256) → critical
/// - Content-Type: image/jpeg → soft (= 改善対象 / 減点)
/// - ETag: "<hex>" が body の SHA256 と一致 → soft
/// - **If-None-Match の 304 は配布版では未実装**。bench は `If-None-Match` を送らない。
async fn image_integrity_checks(
    client: &Client,
    base: &str,
    user: &UserToken,
    cid: &str,
    state: &BenchState,
) -> BenchResult<()> {
    let expected = match state.expected_posted_hash(cid) {
        Some(h) => h,
        None => {
            // bench 自前でない campaign は body hash 比較の対象外 (false positive 回避)
            return Ok(());
        }
    };

    // 200 + body 一致
    let r = fetch_campaign_image(client, base, state, user, cid).await?;
    if r.status != reqwest::StatusCode::OK {
        state.add_critical(format!(
            "GET /api/campaigns/{cid}/image expected 200 got {}",
            r.status
        ));
        return Ok(());
    }
    let body_hash = hex::encode(sha2::Sha256::digest(&r.body));
    if body_hash != expected {
        state.add_critical(format!(
            "GET /api/campaigns/{cid}/image body hash mismatch: expected={expected} got={body_hash}"
        ));
    }
    // Content-Type は soft
    match r.content_type.as_deref() {
        Some(ct) if ct.starts_with("image/jpeg") => {}
        Some(ct) => {
            state.add_soft(format!(
                "GET /api/campaigns/{cid}/image: Content-Type {ct:?} != image/jpeg"
            ));
        }
        None => {
            state.add_soft(format!(
                "GET /api/campaigns/{cid}/image: Content-Type missing"
            ));
        }
    }
    // ETag は soft (期待値は \"<body_hash>\")
    let expected_etag = format!("\"{expected}\"");
    match r.etag.as_deref() {
        Some(e) if e == expected_etag => {}
        Some(e) => {
            state.add_soft(format!(
                "GET /api/campaigns/{cid}/image: ETag {e:?} != expected {expected_etag:?}"
            ));
        }
        None => {
            state.add_soft(format!(
                "GET /api/campaigns/{cid}/image: missing ETag"
            ));
        }
    }

    // 改善検出 probe: If-None-Match に正規 ETag を付けて再 GET。
    // 配布版は If-None-Match を読まず 200、改善後は 304 が返る期待。
    // どちらも OK、それ以外の status は soft 警告。
    let resp = client
        .get(format!("{base}/campaigns/{cid}/image"))
        .header("X-User-ID", &user.id)
        .header(reqwest::header::IF_NONE_MATCH, &expected_etag)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "get image w/ if-none-match", e))?;
    let st = resp.status();
    if st != reqwest::StatusCode::OK && st != reqwest::StatusCode::NOT_MODIFIED {
        state.add_soft(format!(
            "GET /api/campaigns/{cid}/image with If-None-Match: expected 200 or 304, got {st}"
        ));
    }

    Ok(())
}

// === fixture loader ===

fn load_fixtures(dir: &str, limit: Option<usize>) -> Result<Vec<ImageFixture>, String> {
    let started = std::time::Instant::now();
    // entry error は score=0 の運用エラー扱いとして propagate (silently 飲まない)
    let entries_iter =
        std::fs::read_dir(dir).map_err(|e| format!("read_dir({dir}): {e}"))?;
    let mut entries: Vec<PathBuf> = Vec::new();
    for r in entries_iter {
        let entry = r.map_err(|e| format!("read_dir entry({dir}): {e}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jpg") {
            entries.push(path);
        }
    }
    entries.sort();
    if let Some(n) = limit {
        entries.truncate(n);
    }
    if entries.is_empty() {
        return Err(format!("no .jpg fixtures in {dir}"));
    }
    let mut fixtures = Vec::with_capacity(entries.len());
    let mut total_bytes: u64 = 0;
    for path in entries {
        let bytes = std::fs::read(&path).map_err(|e| format!("read {:?}: {e}", path))?;
        if bytes.len() > 204_800 {
            return Err(format!(
                "fixture {:?} exceeds 200 KiB (= API image limit)",
                path
            ));
        }
        if bytes.len() < 3 || bytes[0] != 0xFF || bytes[1] != 0xD8 || bytes[2] != 0xFF {
            return Err(format!("fixture {:?} is not a JPEG (bad magic)", path));
        }
        let hash_hex = hex::encode(sha2::Sha256::digest(&bytes));
        let size = bytes.len();
        total_bytes += size as u64;
        fixtures.push(ImageFixture {
            path,
            hash_hex,
            size,
        });
    }
    eprintln!(
        "bench: load_fixtures took {:?}, n={} total={} MiB",
        started.elapsed(),
        fixtures.len(),
        total_bytes / 1024 / 1024
    );
    Ok(fixtures)
}

fn b64_encode(bytes: &[u8]) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    STANDARD.encode(bytes)
}

// === HTTP helpers ===

async fn create_user(
    client: &Client,
    base: &str,
    state: &BenchState,
    name: &str,
) -> BenchResult<UserToken> {
    let v: Value = client
        .post(format!("{base}/users"))
        .json(&json!({ "name": name }))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "create_user send", e))?
        .error_for_status()
        .map_err(|e| map_status(state, "create_user status", e))?
        .json()
        .await
        .map_err(|e| map_reqwest(state, "create_user json", e))?;
    let id = v["id"]
        .as_str()
        .ok_or_else(|| BenchError::Other("create_user: id missing".to_string()))?
        .to_string();
    Ok(UserToken {
        id,
        name: name.to_string(),
    })
}

/// `GET /api/campaigns` のレスポンスが sort 仕様に沿った順序で並んでいることを検査する。
/// - sort=new : created_at DESC
/// - sort=active : COALESCE(last_joined_at, created_at) DESC
///
/// RFC3339 + 固定幅 UTC (`...Z`) 前提で文字列の辞書順比較は時系列と一致する。
/// 違反は最初の 1 件だけ critical にする (= 同じ list で連発させない)。
fn sort_key<'a>(c: &'a CampaignSnap, sort: &str) -> &'a str {
    match sort {
        "active" => c.last_joined_at.as_deref().unwrap_or(&c.created_at),
        _ => &c.created_at,
    }
}

fn assert_sort_order(state: &BenchState, list: &[CampaignSnap], sort: &str, ctx: &str) {
    for w in list.windows(2) {
        let (prev, next) = (&w[0], &w[1]);
        let pk = sort_key(prev, sort);
        let nk = sort_key(next, sort);
        if pk < nk {
            state.add_critical(format!(
                "{ctx}: sort={sort} order violation: prev id={} key={:?} < next id={} key={:?}",
                prev.id, pk, next.id, nk
            ));
            return;
        }
    }
}

/// `GET /api/campaigns` (sort=new / sort=active) は status=open のみ返すべき仕様。
/// 違反は最初の 1 件だけ critical。
fn assert_status_open_only(state: &BenchState, list: &[CampaignSnap], ctx: &str) {
    for c in list {
        if c.status != "open" {
            state.add_critical(format!(
                "{ctx}: list contains non-open campaign: id={} status={:?}",
                c.id, c.status
            ));
            return;
        }
    }
}

async fn list_campaigns(
    client: &Client,
    base: &str,
    state: &BenchState,
    user: &UserToken,
    tags: &str,
    sort: &str,
) -> BenchResult<Vec<CampaignSnap>> {
    let mut q: Vec<(&str, String)> = vec![("sort", sort.to_string())];
    if !tags.is_empty() {
        q.push(("tags", tags.to_string()));
    }
    let r: Vec<CampaignSnap> = client
        .get(format!("{base}/campaigns"))
        .query(&q)
        .header("X-User-ID", &user.id)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "list_campaigns send", e))?
        .error_for_status()
        .map_err(|e| map_status(state, "list_campaigns status", e))?
        .json()
        .await
        .map_err(|e| map_reqwest(state, "list_campaigns json", e))?;
    Ok(r)
}

async fn get_campaign(
    client: &Client,
    base: &str,
    state: &BenchState,
    user: &UserToken,
    id: &str,
) -> BenchResult<CampaignSnap> {
    let r: CampaignSnap = client
        .get(format!("{base}/campaigns/{id}"))
        .header("X-User-ID", &user.id)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "get_campaign send", e))?
        .error_for_status()
        .map_err(|e| map_status(state, "get_campaign status", e))?
        .json()
        .await
        .map_err(|e| map_reqwest(state, "get_campaign json", e))?;
    Ok(r)
}

async fn create_campaign(
    client: &Client,
    base: &str,
    state: &BenchState,
    user: &UserToken,
    body: &Value,
) -> BenchResult<CampaignSnap> {
    // fixture から 1 枚ピックして body に image (base64) を注入する
    let fixture = state.pick_fixture();
    let bytes = std::fs::read(&fixture.path)
        .map_err(|e| BenchError::Other(format!("read fixture {:?}: {e}", fixture.path)))?;
    let img_b64 = b64_encode(&bytes);
    let mut body = body.clone();
    if let Some(obj) = body.as_object_mut() {
        obj.insert("image".to_string(), Value::String(img_b64));
    } else {
        return Err(BenchError::Other(
            "create_campaign: body must be a JSON object".into(),
        ));
    }

    let r: CampaignSnap = client
        .post(format!("{base}/campaigns"))
        .header("X-User-ID", &user.id)
        .json(&body)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "create_campaign send", e))?
        .error_for_status()
        .map_err(|e| map_status(state, "create_campaign status", e))?
        .json()
        .await
        .map_err(|e| map_reqwest(state, "create_campaign json", e))?;

    // bench 自前 campaign の (id → 期待 fixture hash) を覚える。
    // 後段で GET /api/campaigns/{id}/image の body bytes と SHA256 一致を critical 検証する。
    state.register_posted(&r.id, &fixture.hash_hex);

    Ok(r)
}

struct ImageResp {
    status: reqwest::StatusCode,
    body: Vec<u8>,
    etag: Option<String>,
    content_type: Option<String>,
}

/// GET /api/campaigns/{id}/image を発行し、status/body/etag/content_type を返す。
async fn fetch_campaign_image(
    client: &Client,
    base: &str,
    state: &BenchState,
    user: &UserToken,
    id: &str,
) -> BenchResult<ImageResp> {
    let resp = client
        .get(format!("{base}/campaigns/{id}/image"))
        .header("X-User-ID", &user.id)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "get image send", e))?;
    let status = resp.status();
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = resp
        .bytes()
        .await
        .map_err(|e| map_reqwest(state, "get image read body", e))?
        .to_vec();
    Ok(ImageResp {
        status,
        body,
        etag,
        content_type,
    })
}

/// `join_after_me_check` の戻り値。
///
/// docs/authoring/design.md §4.1.1 (b) は「409 (race による close / 既参加) は許容」と
/// 明記しているので、helper 契約としても 409 は呼び出し側に判断を委ねる。
/// (= 現 integrity では「想定外」として呼び出し側が Err 化する)
enum JoinChecked {
    Joined(CampaignSnap),
    Conflict,
}

/// /api/me を取った直後に join を発行し、不変条件 (b) を check する:
/// /api/me で残高十分 (credit_used + price ≤ credit_limit) を確認した状態で 402 が返ったら critical。
///
/// (b) は monotonicity に依拠する: 同 user が他 join を発行しないあいだ credit_used は単調
/// 非増加 (refund のみ起きうる) → /api/me 観測時の十分性は後続 join 時にも維持される。
/// この helper は 1 task / 1 user / 直近の他 join なし の前提で呼ぶこと。
///
/// price は呼び出し側が直接渡すと誤指定で false positive を生むので CampaignSnap を渡し、
/// 比較は `i64` 昇格で overflow を排除する。
async fn join_after_me_check(
    client: &Client,
    base: &str,
    user: &UserToken,
    state: &BenchState,
    campaign: &CampaignSnap,
    ctx: &str,
) -> BenchResult<JoinChecked> {
    let me = get_me(client, base, user, state, ctx).await?;
    let sufficient = i64::from(me.credit_used) + i64::from(campaign.price)
        <= i64::from(me.credit_limit);
    let resp = client
        .post(format!("{base}/campaigns/{}/join", campaign.id))
        .header("X-User-ID", &user.id)
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "join send", e))?;
    let status = resp.status();
    if sufficient && status == reqwest::StatusCode::PAYMENT_REQUIRED {
        state.add_critical(format!(
            "(b) /api/me sufficient → join 402 at {ctx}: user={} credit_used={} price={} credit_limit={}",
            user.id, me.credit_used, campaign.price, me.credit_limit
        ));
        return Err(BenchError::Other(format!("(b) violation at {ctx}")));
    }
    if status == reqwest::StatusCode::CONFLICT {
        return Ok(JoinChecked::Conflict);
    }
    let body: CampaignSnap = resp
        .error_for_status()
        .map_err(|e| map_status(state, "join status", e))?
        .json()
        .await
        .map_err(|e| map_reqwest(state, "join json", e))?;
    Ok(JoinChecked::Joined(body))
}

async fn create_saved_search(
    client: &Client,
    base: &str,
    state: &BenchState,
    user: &UserToken,
    tags: &[&str],
) -> BenchResult<()> {
    let resp = client
        .post(format!("{base}/saved_searches"))
        .header("X-User-ID", &user.id)
        .json(&json!({ "tags": tags }))
        .send()
        .await
        .map_err(|e| map_reqwest(state, "saved_search send", e))?;
    if !resp.status().is_success() {
        // 想定外 status は §6.5 に従い soft 計上 + scenario abort (続行不能のため)。
        state.add_soft(format!(
            "saved_search unexpected status: {}",
            resp.status()
        ));
        return Err(BenchError::Soft("saved_search status".to_string()));
    }
    Ok(())
}

async fn list_charges(
    client: &Client,
    base: &str,
    state: &BenchState,
    user: &UserToken,
) -> BenchResult<Vec<ChargeSnap>> {
    let r: Vec<ChargeSnap> = client
        .get(format!("{base}/charges"))
        .header("X-User-ID", &user.id)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "charges send", e))?
        .error_for_status()
        .map_err(|e| map_status(state, "charges status", e))?
        .json()
        .await
        .map_err(|e| map_reqwest(state, "charges json", e))?;
    Ok(r)
}

/// GET /api/me を呼び、credit_used > credit_limit の場合は不変条件 (a) 違反として
/// state.add_critical する (= 「観測の度に必ず (a) を assert」を helper 内に閉じる)。
async fn get_me(
    client: &Client,
    base: &str,
    user: &UserToken,
    state: &BenchState,
    ctx: &str,
) -> BenchResult<MeSnap> {
    let me: MeSnap = client
        .get(format!("{base}/me"))
        .header("X-User-ID", &user.id)
        .send()
        .await
        .map_err(|e| map_reqwest(state, "get_me send", e))?
        .error_for_status()
        .map_err(|e| map_status(state, "get_me status", e))?
        .json()
        .await
        .map_err(|e| map_reqwest(state, "get_me json", e))?;
    if me.credit_used > me.credit_limit {
        state.add_critical(format!(
            "(a) credit_used > credit_limit at {ctx}: user={} credit_used={} credit_limit={}",
            user.id, me.credit_used, me.credit_limit
        ));
    }
    Ok(me)
}

// === load phase ===

/// load 中の固定走行時間 + 12f 方式の成功カウンタ階段で audited / notification actor を駆動。
///
/// 並列度は固定ではなく、load 内 join 成功 (= 200 OK) のたびに `BenchState::record_load_join_success`
/// で進む `load_join_success_count` を trigger に、`BENCH_RAMP_STEP_JOINS` 件ごとに
/// 各 actor 種を 1 体ずつ spawn (それぞれの `*_MAX` cap まで)。捌けなければ階段は止まる。
///
/// audited actor: 1 user 1 worker, in-flight ≤ 1 で
///   GET /api/campaigns?sort={new,active} → /api/me で買える candidate 選択 → POST /join。
///   sort=new は新着発掘 / sort=active は人気駆動の 2 ペルソナ ([audited_actor_new] /
///   [audited_actor_active] 参照)。既存 helper (a)(b)(local invariants) を全経路で発火させる。
/// notification actor: saved_searches 1〜3 件登録、bench webhook receiver 経由で
///   通知を受けて join。/api/me sufficient → 402 critical も同 helper で発火。
///
/// audited と notification は user を完全分離する (= (b) monotonicity 前提を維持)。
async fn run_load_phase(
    client: &Client,
    base: &str,
    state: Arc<BenchState>,
) -> BenchResult<()> {
    let duration_secs = env::var("BENCH_LOAD_DURATION_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(60);
    let cap_new = env::var("BENCH_AUDITED_NEW_ACTORS_MAX")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3);
    let cap_active = env::var("BENCH_AUDITED_ACTIVE_ACTORS_MAX")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(5);
    let cap_notif = env::var("BENCH_NOTIFICATION_ACTORS_MAX")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(4);
    // initial=0 だと初期 spawn が起きず join 成功も発生しない → ramp が永遠に進まない
    // dead load になるので必ず 1 以上に潰す (= ramp_step と同系統の入力ガード)。
    let initial_raw = env::var("BENCH_INITIAL_ACTORS_PER_KIND")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);
    let initial = initial_raw.max(1);
    if initial_raw == 0 {
        eprintln!("load: BENCH_INITIAL_ACTORS_PER_KIND=0 is invalid for ramp; using 1");
    }
    // step=0 は `while count >= next_threshold { next_threshold += step }` で
    // 無限ループ (= bench がハング) になるので必ず 1 以上に潰す。
    let ramp_step = env::var("BENCH_RAMP_STEP_JOINS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(5)
        .max(1);

    // 全 cap=0 は静かな 60s 待ち (= bench 設定ミス) になるので早期に落とす。
    // kind 個別の cap=0 は「その kind を無効化」として自然な意味を持つので許容。
    if cap_new == 0 && cap_active == 0 && cap_notif == 0 {
        return Err(BenchError::Other(
            "load: all actor caps are 0; at least one BENCH_*_ACTORS_MAX must be > 0".into(),
        ));
    }

    eprintln!(
        "load: duration={}s caps(new/active/notif)={}/{}/{} initial={} ramp_step={}",
        duration_secs, cap_new, cap_active, cap_notif, initial, ramp_step
    );

    // run_load_phase 固有の deadline cancel は cancel_global の child。
    // - deadline に達したら cancel.cancel() で actor だけ停止 → finalcheck まで進む
    // - 50-soft 到達で state.cancel_global がキャンセルされると child も連動キャンセル
    //   → actor 停止、deadline sleep も select! で即時抜ける、finalcheck は冒頭 guard で skip
    let cancel = state.cancel_global.child_token();

    // seller actor: 1 約定観測 → 2 件出品。ramp とは独立に 1 体固定で常駐。
    // bounded channel (容量 1024) で notify_load_close からの close event を受ける。
    // 容量超過は drop 扱い (= seller_queue_dropped 計上)、bench 側 backlog 保存で負荷形状が
    // 歪むのを防ぐ (smart-friend レビュー方針)。
    let (seller_tx, seller_rx) = mpsc::channel::<()>(1024);
    state.install_load_close_channel(seller_tx);
    let seller = {
        let client = client.clone();
        let base = base.to_string();
        let state = state.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            // create_user の inflight 中に cancel が来たら即時抜ける (= ramp の abort_all 設計と
            // 同等の wall-clock 守り)。await を裸で残すと reqwest の per-req timeout
            // (REQUEST_TIMEOUT = 10s) まで待つ恐れがある。
            let user = tokio::select! {
                _ = cancel.cancelled() => return,
                res = create_user(&client, &base, &state, "load-seller") => match res {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("seller: create_user failed: {e}");
                        return;
                    }
                },
            };
            seller_actor(&client, &base, &user, &state, seller_rx, cancel).await;
        })
    };

    let controller = tokio::spawn(ramp_controller(
        client.clone(),
        base.to_string(),
        state.clone(),
        cancel.clone(),
        cap_new,
        cap_active,
        cap_notif,
        initial,
        ramp_step,
    ));

    // 固定時間 or 50-soft 到達 (= cancel_global → child 連動) のどちらか早い方で終了。
    // sleep を select! 化しないと、50 件刺さってから残り deadline 分待ってしまう。
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(duration_secs)) => {
            eprintln!("load: deadline reached, cancelling actors");
            cancel.cancel();
        }
        _ = cancel.cancelled() => {
            eprintln!("load: cancellation propagated (likely soft-fail threshold), aborting actors");
        }
    }

    // controller 内で「cancel 観測 → 即時 abort_all + join」まで完結する設計
    // (走行 wall-clock を予測可能にするため grace は撤去、commit 2c8d4da)。
    // await は controller のみ。
    let _ = controller.await;

    // load actors の sender を全て drop (= channel close、receiver loop 終了済)。
    state.notification_routes.lock().unwrap().clear();
    // seller の sender も drop (close_load_close_channel で BenchState から外す)
    // → seller actor は cancel か rx.recv()=None で抜ける。
    state.close_load_close_channel();
    let _ = seller.await;
    Ok(())
}

/// 12f 方式の成功カウンタ階段を駆動するコントローラ。
///
/// 設計:
/// - JoinSet を 1 task で所有 (= cancel / drain / abort の責務を 1 箇所に閉じる)。
/// - 200ms 周期で `state.load_join_success_count` をポーリング。`count >= next_threshold` のたび
///   各 actor 種について `current < cap` なら 1 体ずつ追加 spawn し、`next_threshold += ramp_step`。
/// - 同 select! 内で `joinset.join_next()` も拾い、actor の早期 return / panic をログ化。
///   spawn 数と alive 数のズレが見えるようにしておく (cap 数とは別軸)。
/// - cancel 観測で即時 abort_all → join (走行 wall-clock 予測のため grace なし、commit 2c8d4da)。
#[allow(clippy::too_many_arguments)]
async fn ramp_controller(
    client: Client,
    base: String,
    state: Arc<BenchState>,
    cancel: CancellationToken,
    cap_new: usize,
    cap_active: usize,
    cap_notif: usize,
    initial: usize,
    ramp_step: u64,
) {
    let mut joinset: JoinSet<()> = JoinSet::new();
    let mut audited_new = 0usize;
    let mut audited_active = 0usize;
    let mut notification = 0usize;
    let mut next_threshold: u64 = ramp_step;
    let mut caps_reached_logged = false;

    // 初期 spawn (initial 個ずつ、cap 内で)。spawn 後にカウンタの再評価が要らないので
    // ramp loop に入る前にまとめて spawn する。
    for _ in 0..initial {
        if audited_new < cap_new {
            spawn_audited_new(
                &mut joinset,
                client.clone(),
                base.clone(),
                state.clone(),
                cancel.clone(),
                audited_new,
                0,
                0,
            );
            audited_new += 1;
        }
        if audited_active < cap_active {
            spawn_audited_active(
                &mut joinset,
                client.clone(),
                base.clone(),
                state.clone(),
                cancel.clone(),
                audited_active,
                0,
                0,
            );
            audited_active += 1;
        }
        if notification < cap_notif {
            spawn_notification(
                &mut joinset,
                client.clone(),
                base.clone(),
                state.clone(),
                cancel.clone(),
                notification,
                0,
                0,
            );
            notification += 1;
        }
    }

    // 初期 spawn だけで全 cap に到達した場合 (例: cap=initial) は ramp 判定が
    // 1 度も spawn しないので、観測性のためここでログを出しておく。
    if audited_new >= cap_new && audited_active >= cap_active && notification >= cap_notif {
        eprintln!("ramp: all caps reached at count=0 (initial spawn already filled all kinds)");
        caps_reached_logged = true;
    }

    let mut tick = tokio::time::interval(Duration::from_millis(200));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // interval の 1 回目は即時発火するので捨てる。これで 1 回目の ramp 判定は
    // 起動から 200ms 後にずれ、初期 actor が立ち上がる猶予になる。
    // (`next_threshold = ramp_step.max(1)` のため count=0 で while 内に入る心配は無く、
    //  純粋に「load 開始直後 200ms 内の判定タイミングを後ろに倒す」効果のみ)
    tick.tick().await;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tick.tick() => {
                if caps_reached_logged {
                    // 全 cap 到達後はカウンタを進めても spawn は起きない。
                    // tick での threshold check 自体を止め、actor 完了の回収のみ続ける。
                    continue;
                }
                let count = state.load_join_success_count.load(Ordering::Relaxed);
                while count >= next_threshold {
                    let mut spawned_any = false;
                    if audited_new < cap_new {
                        spawn_audited_new(
                            &mut joinset,
                            client.clone(),
                            base.clone(),
                            state.clone(),
                            cancel.clone(),
                            audited_new,
                            count,
                            next_threshold,
                        );
                        audited_new += 1;
                        spawned_any = true;
                    }
                    if audited_active < cap_active {
                        spawn_audited_active(
                            &mut joinset,
                            client.clone(),
                            base.clone(),
                            state.clone(),
                            cancel.clone(),
                            audited_active,
                            count,
                            next_threshold,
                        );
                        audited_active += 1;
                        spawned_any = true;
                    }
                    if notification < cap_notif {
                        spawn_notification(
                            &mut joinset,
                            client.clone(),
                            base.clone(),
                            state.clone(),
                            cancel.clone(),
                            notification,
                            count,
                            next_threshold,
                        );
                        notification += 1;
                        spawned_any = true;
                    }
                    if !spawned_any {
                        eprintln!("ramp: all caps reached at count={count}");
                        caps_reached_logged = true;
                        break;
                    }
                    next_threshold += ramp_step;
                }
            }
            Some(res) = joinset.join_next(), if !joinset.is_empty() => {
                if let Err(e) = res {
                    // panic / cancel で actor task が落ちた。spawn 数 (audited_new 等) と
                    // 実 alive 数のズレを可視化するためログだけ残す (再 spawn はしない)。
                    eprintln!("ramp: actor task ended: {e}");
                }
            }
        }
    }

    // grace なし: cancel 観測後は in-flight HTTP を即時 abort する (commit 2c8d4da)。
    // per-req timeout (REQUEST_TIMEOUT) は走行全体の wall-clock 上限を予測可能にするための
    // ものであって、shutdown drain には待たない。webapp 側は連続切断を見るが docs/manual.md
    // §6.3 の許容範囲内。abort 後の join は panic / cancelled を回収する保険。
    joinset.abort_all();
    while let Some(res) = joinset.join_next().await {
        if let Err(e) = res {
            eprintln!("ramp: actor task ended (drain): {e}");
        }
    }
}

/// audited.new actor を 1 体 spawn。create_user の失敗は単体落ちとして許容する
/// (cap 数と alive 数のズレは ramp の `actor task ended` ログで可視化される)。
#[allow(clippy::too_many_arguments)]
fn spawn_audited_new(
    joinset: &mut JoinSet<()>,
    client: Client,
    base: String,
    state: Arc<BenchState>,
    cancel: CancellationToken,
    idx: usize,
    count: u64,
    threshold: u64,
) {
    eprintln!("ramp: spawn audited.new[{idx}] (count={count}, threshold={threshold})");
    joinset.spawn(async move {
        let user = match create_user(&client, &base, &state, &format!("audited-new-{idx}")).await {
            Ok(u) => u,
            Err(e) => {
                eprintln!("ramp: audited.new[{idx}] create_user failed: {e}");
                return;
            }
        };
        audited_actor_new(&client, &base, &user, &state, cancel).await;
    });
}

/// audited.active actor を 1 体 spawn。
#[allow(clippy::too_many_arguments)]
fn spawn_audited_active(
    joinset: &mut JoinSet<()>,
    client: Client,
    base: String,
    state: Arc<BenchState>,
    cancel: CancellationToken,
    idx: usize,
    count: u64,
    threshold: u64,
) {
    eprintln!("ramp: spawn audited.active[{idx}] (count={count}, threshold={threshold})");
    joinset.spawn(async move {
        let user = match create_user(&client, &base, &state, &format!("audited-active-{idx}")).await {
            Ok(u) => u,
            Err(e) => {
                eprintln!("ramp: audited.active[{idx}] create_user failed: {e}");
                return;
            }
        };
        audited_actor_active(&client, &base, &user, &state, cancel).await;
    });
}

/// notification actor を 1 体 spawn。create_user → saved_search 登録 → channel 登録 → 受信 loop。
/// channel sender は notification_routes に登録。actor 完了で sender drop されると
/// run_load_phase 末尾の clear で残りも drop される (どちら経由でも receiver loop は閉じる)。
#[allow(clippy::too_many_arguments)]
fn spawn_notification(
    joinset: &mut JoinSet<()>,
    client: Client,
    base: String,
    state: Arc<BenchState>,
    cancel: CancellationToken,
    idx: usize,
    count: u64,
    threshold: u64,
) {
    eprintln!("ramp: spawn notification[{idx}] (count={count}, threshold={threshold})");
    joinset.spawn(async move {
        let user = match create_user(&client, &base, &state, &format!("notif-{idx}")).await {
            Ok(u) => u,
            Err(e) => {
                eprintln!("ramp: notification[{idx}] create_user failed: {e}");
                return;
            }
        };
        if let Err(e) = register_random_saved_searches(&client, &base, &user, &state).await {
            eprintln!("ramp: notification[{idx}] register_saved_searches failed: {e}");
            return;
        }
        // user 別 channel (容量は notification 規模に対し十分。fill 時は drop = 仕様 OK)。
        let (tx, rx) = mpsc::channel::<String>(64);
        state
            .notification_routes
            .lock()
            .unwrap()
            .insert(user.id.clone(), tx);
        notification_actor(&client, &base, &user, &state, rx, cancel).await;
    });
}

/// notification user に random tag の saved_search を 1〜3 件登録。
/// 「saved_search が存在する user」だけが webhook 配送先になるので、必ず 1 件は入れる。
async fn register_random_saved_searches(
    client: &Client,
    base: &str,
    user: &UserToken,
    state: &BenchState,
) -> BenchResult<()> {
    let n = {
        let mut rng = state.rng.lock().unwrap();
        rng.gen_range(1..=3usize)
    };
    let tags = seed_data::TAGS;
    let mut pool: Vec<&'static str> = tags.iter().map(|t| t.name).collect();
    {
        let mut rng = state.rng.lock().unwrap();
        pool.shuffle(&mut *rng);
    }
    for tag in pool.iter().take(n) {
        // 1 saved_search あたり tag は 1 つで十分 (idea.md 仕様: ≤ 3 / 重複不可)。
        // ここは「マッチする集合が広いほど通知が来やすい」を狙って単一 tag に倒す。
        create_saved_search(client, base, state, user, &[*tag]).await?;
    }
    Ok(())
}

/// audited actor (sort=new): 新着発掘ペルソナ。
/// sort=new は created_at 降順なので、新規作成された campaign を高速に拾い上げる。
/// sort=active と並走させて参加分布を分散させ、特定の campaign に偏らないようにする。
async fn audited_actor_new(
    client: &Client,
    base: &str,
    user: &UserToken,
    state: &BenchState,
    cancel: CancellationToken,
) {
    audited_actor_loop(client, base, user, state, cancel, "new").await
}

/// audited actor (sort=active): 人気駆動ペルソナ。
/// 仕様上 0 参加 campaign も結果に含まれる (= 仕様の帰結としては「人気優先」にはならない)。
/// 本 actor は credit 内の candidate を一様ランダムに pick するが、Load 中は join されるたびに
/// 当該 campaign の last_joined_at が更新されて active 上位に押し上がる動的均衡が働き、
/// 結果として参加実績のある campaign が close まで押し進む傾向になる。
async fn audited_actor_active(
    client: &Client,
    base: &str,
    user: &UserToken,
    state: &BenchState,
    cancel: CancellationToken,
) {
    audited_actor_loop(client, base, user, state, cancel, "active").await
}

/// audited actor 共通ループ: sort 軸だけが違う 2 種類の persona を 1 関数で扱う。
/// 1 user 1 worker、in-flight ≤ 1 で credit を尊重する join 行動。
/// (a) (b) は呼び出した get_me / join_after_me_check 内で発火する。
async fn audited_actor_loop(
    client: &Client,
    base: &str,
    user: &UserToken,
    state: &BenchState,
    cancel: CancellationToken,
    sort: &'static str,
) {
    let label = format!("audited.{sort}[{}]", user.name);
    while !cancel.is_cancelled() {
        // 1) GET /api/campaigns?sort={new,active} (status=open のみ返る仕様)
        let campaigns = match list_campaigns(client, base, state, user, "", sort).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{label}: list_campaigns: {e}");
                if sleep_or_cancel(&cancel, Duration::from_millis(500)).await {
                    return;
                }
                continue;
            }
        };
        for c in &campaigns {
            state.record_campaign_observation(c, Phase::Load);
        }
        if cancel.is_cancelled() {
            return;
        }

        // 2) /api/me で残高を観測 (= (a) check が helper 内で発火)
        let me = match get_me(client, base, user, state, &label).await {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{label}: get_me: {e}");
                if sleep_or_cancel(&cancel, Duration::from_millis(500)).await {
                    return;
                }
                continue;
            }
        };

        // 3) candidate 選択: open かつ buy 可能。snapshot を持って lock 短時間で抜ける。
        let candidates: Vec<CampaignSnap> = campaigns
            .into_iter()
            .filter(|c| c.status == "open" && c.current_count < c.goal_count)
            .filter(|c| {
                i64::from(me.credit_used) + i64::from(c.price) <= i64::from(me.credit_limit)
            })
            .collect();
        if candidates.is_empty() {
            // think time。candidate 不在の指標は load calibration 用に将来集計予定。
            if sleep_or_cancel(&cancel, Duration::from_millis(200)).await {
                return;
            }
            continue;
        }
        let pick = {
            let mut rng = state.rng.lock().unwrap();
            candidates[rng.gen_range(0..candidates.len())].clone()
        };

        // 4) join_after_me_check (b) check 経由で join 発行
        match join_after_me_check(client, base, user, state, &pick, &label).await {
            Ok(JoinChecked::Joined(snap)) => {
                state.record_campaign_observation(&snap, Phase::Load);
                // ramp 階段の trigger (= load 限定の join 成功カウンタ)
                state.record_load_join_success();
            }
            Ok(JoinChecked::Conflict) => {
                // 別 actor が先に close した / 既参加 → 仕様 OK、無視
            }
            Err(e) => {
                // (b) 違反は state.add_critical 済。それ以外の send/json error はここに来る。
                eprintln!("{label}: join: {e}");
            }
        }
    }
}

/// notification actor: bench webhook receiver から自分宛通知を受信し、
/// 通知された campaign に対し /api/me 確認 → join。
async fn notification_actor(
    client: &Client,
    base: &str,
    user: &UserToken,
    state: &BenchState,
    mut rx: mpsc::Receiver<String>,
    cancel: CancellationToken,
) {
    let label = format!("notif[{}]", user.name);
    loop {
        let cid = tokio::select! {
            _ = cancel.cancelled() => return,
            msg = rx.recv() => match msg {
                Some(c) => c,
                None => return,  // sender drop (shutdown 経路)
            }
        };
        // GET /api/campaigns/{id} で snap を取得 (price / status 等の判定材料)
        let snap = match get_campaign(client, base, state, user, &cid).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{label}: get_campaign({cid}): {e}");
                continue;
            }
        };
        state.record_campaign_observation(&snap, Phase::Load);
        if snap.status != "open" {
            continue;
        }
        // (b) check 経由で join
        match join_after_me_check(client, base, user, state, &snap, &label).await {
            Ok(JoinChecked::Joined(after)) => {
                state.record_campaign_observation(&after, Phase::Load);
                state.record_load_join_success();
            }
            Ok(JoinChecked::Conflict) => {}
            Err(e) => {
                eprintln!("{label}: join: {e}");
            }
        }
    }
}

/// `tokio::time::sleep` を cancel 待ちと並走させる。
/// 戻り値: true なら cancel 発火、false なら sleep 完了で続行。
async fn sleep_or_cancel(cancel: &CancellationToken, dur: Duration) -> bool {
    tokio::select! {
        _ = cancel.cancelled() => true,
        _ = tokio::time::sleep(dur) => false,
    }
}

/// seller actor: load phase 中の close 初観測 1 件につき新規 campaign を逐次 2 件出品する。
/// 狙いは「約定が増えるほど在庫も増える」単調増加で、seed pool 枯渇による candidate 干渉を防ぐ。
///
/// 設計:
/// - event 受信ごとに `SELLER_CAMPAIGNS_PER_EVENT` 件 (= 2) を逐次 create_campaign。
///   2 並列にすると create_campaign の image base64 POST が join hot path とは別軸の負荷を
///   急に太らせるため、初期実装は逐次で開始する (smart-friend レビュー方針)。
/// - tag は `seed_data::TAGS` 全体から 1〜3 個ランダム抽出。pretest / integrity が固定する
///   `mesh` / `ergonomic` だけに偏らないことで sort=active への 0 参加新規汚染を分散させる。
/// - price は `seed_data::PRICES` から uniform、goal_count は 2〜6 に寄せる
///   (= load 中に再 close されやすい範囲、レビューの「在庫補充」方針)。
/// - 出品結果 snapshot は `record_campaign_observation` で観測する
///   (= 0 参加新規 active 上位の局所不変性チェックも自動的に網にかける)。
const SELLER_CAMPAIGNS_PER_EVENT: usize = 2;

async fn seller_actor(
    client: &Client,
    base: &str,
    user: &UserToken,
    state: &BenchState,
    mut rx: mpsc::Receiver<()>,
    cancel: CancellationToken,
) {
    let label = format!("seller[{}]", user.name);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            msg = rx.recv() => {
                if msg.is_none() {
                    return; // sender drop (run_load_phase 終了経路)
                }
            }
        }

        for _ in 0..SELLER_CAMPAIGNS_PER_EVENT {
            if cancel.is_cancelled() {
                return;
            }
            let body = make_seller_campaign_body(state);
            // create_campaign の inflight 中に cancel が来たら裸の `.await` だと最大
            // REQUEST_TIMEOUT (10s) 待つ。ramp の abort_all 設計に合わせて即時抜ける。
            let res = tokio::select! {
                _ = cancel.cancelled() => return,
                res = create_campaign(client, base, state, user, &body) => res,
            };
            match res {
                Ok(snap) => {
                    state
                        .seller_campaigns_created
                        .fetch_add(1, Ordering::Relaxed);
                    state.record_campaign_observation(&snap, Phase::Load);
                }
                Err(e) => {
                    state.seller_create_errors.fetch_add(1, Ordering::Relaxed);
                    eprintln!("{label}: create_campaign: {e}");
                    // think time。ここで break しないのは、event 1 つあたりの
                    // 「2 件出品」契約をなるべく満たすため。
                    if sleep_or_cancel(&cancel, Duration::from_millis(200)).await {
                        return;
                    }
                }
            }
        }
    }
}

/// seller actor が出品する campaign body を生成する。決定論性のため bench 全体共有 PRNG を使う。
/// price / goal_count / tag 分布は smart-friend レビューに従い:
/// - price: `seed_data::PRICES` から uniform
/// - goal_count: 2〜6 (= 在庫補充目的、過大な goal は close 率を落とす)
/// - tags: `seed_data::TAGS` 全体から 1〜3 件、`mesh`/`ergonomic` 偏重を避ける
fn make_seller_campaign_body(state: &BenchState) -> Value {
    let mut rng = state.rng.lock().unwrap();
    let price = *seed_data::PRICES
        .choose(&mut *rng)
        .expect("seed_data::PRICES is non-empty");
    let goal_count = rng.gen_range(2..=6);
    let tag_count = rng.gen_range(1..=3usize);
    let mut pool: Vec<&'static str> = seed_data::TAGS.iter().map(|t| t.name).collect();
    pool.shuffle(&mut *rng);
    let tags: Vec<&str> = pool.into_iter().take(tag_count).collect();
    json!({
        "name": "load-seller campaign",
        "description": "load-seller generated campaign",
        "price": price,
        "goal_count": goal_count,
        "tags": tags,
    })
}

// === finalcheck (charges 突合) ===

/// 観測 closed campaign の participants 全員に対し /api/charges を確認し、
/// 課金漏れ / 二重課金 critical を検出する。同一 user は 1 回だけ取得しキャッシュ。
async fn finalcheck(client: &Client, base: &str, state: &BenchState) -> BenchResult<()> {
    // 50-soft 到達済みの場合は score=0 確定なので charges 突合まで進めず early return。
    // load actor 強制 abort 直後の中途半端な観測 / timeout 多発状態に対する
    // false positive critical を避ける (FAIL 原因をシングルソースに保つ)。
    if state.soft_count.load(Ordering::Relaxed) >= SOFT_FAIL_THRESHOLD {
        eprintln!("finalcheck: skipped (soft-fail threshold {SOFT_FAIL_THRESHOLD} reached)");
        return Ok(());
    }

    let snapshot: Vec<(String, Vec<String>)> = {
        let closed = state.closed_campaigns.lock().unwrap();
        closed
            .iter()
            .map(|(cid, rec)| (cid.clone(), rec.participants.clone()))
            .collect()
    };

    if snapshot.is_empty() {
        eprintln!("finalcheck: no closed campaigns observed (load phase produced 0 closes)");
        return Ok(());
    }

    // unique participants (= 全 closed campaign の参加者集合) からランダムに最大
    // FINALCHECK_SAMPLE_SIZE 人を抽出し、その user の /charges のみ突合する。
    // 検出はサンプル運に依存するが、走行全体の wall-clock を unique users 数から切り離す。
    //
    // 決定性: HashSet -> Vec の順序は HashMap の RandomState 由来で process-local に揺れる。
    // 同じ BENCH_RNG_SEED で sample を再現するため、Vec 化後に sort_unstable() で正規化してから
    // shuffle する。
    let unique_users: Vec<String> = {
        let mut set: HashSet<String> = HashSet::new();
        for (_, parts) in &snapshot {
            for u in parts {
                set.insert(u.clone());
            }
        }
        let mut users: Vec<String> = set.into_iter().collect();
        users.sort_unstable();
        users
    };
    if unique_users.is_empty() {
        // record_campaign_observation で participants は必ず非空 (= goal_count >= 2) なので
        // 実害ある経路ではないが、防御的にログだけ出して抜ける。
        eprintln!("finalcheck: closed campaigns have no participants, skipping");
        return Ok(());
    }
    let sample: Vec<String> = {
        let mut pool = unique_users.clone();
        let mut rng = state.rng.lock().unwrap();
        pool.shuffle(&mut *rng);
        pool.truncate(FINALCHECK_SAMPLE_SIZE);
        pool
    };
    let sampled_set: HashSet<&str> = sample.iter().map(|s| s.as_str()).collect();

    let mut charges_cache: HashMap<String, Vec<ChargeSnap>> = HashMap::new();
    for uid in &sample {
        // X-User-ID 認証は user_id 文字列を直接使うので、ここで擬似 UserToken を組む。
        let token = UserToken {
            id: uid.clone(),
            name: format!("finalcheck:{uid}"),
        };
        let charges = list_charges(client, base, state, &token).await?;
        charges_cache.insert(uid.clone(), charges);
    }

    for (cid, participants) in &snapshot {
        for uid in participants {
            if !sampled_set.contains(uid.as_str()) {
                continue;
            }
            let charges = &charges_cache[uid];
            let count = charges.iter().filter(|ch| &ch.campaign.id == cid).count();
            if count == 0 {
                state.add_critical(format!(
                    "finalcheck: missing charge: user={uid} campaign={cid}"
                ));
            } else if count > 1 {
                state.add_critical(format!(
                    "finalcheck: double charge: user={uid} campaign={cid} count={count}"
                ));
            }
        }
    }
    eprintln!(
        "finalcheck: closed_campaigns={} unique_users_total={} sampled={}",
        snapshot.len(),
        unique_users.len(),
        sample.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_score_zero_zero_zero() {
        assert_eq!(compute_score(0, 0, 0), 0);
    }

    #[test]
    fn compute_score_no_errors() {
        assert_eq!(compute_score(0, 0, 5_000), 5_000);
    }

    #[test]
    fn compute_score_partial_soft_clamp_to_zero() {
        // 1 件 減点 で素点より大きい → 0 にクランプ
        assert_eq!(compute_score(0, 1, 0), 0);
    }

    #[test]
    fn compute_score_soft_5_total_1000() {
        // 1000 - 5*100 = 500
        assert_eq!(compute_score(0, 5, 1_000), 500);
    }

    #[test]
    fn compute_score_soft_49_threshold_boundary_below() {
        // 49 件までは FAIL にならず減点のみ
        assert_eq!(compute_score(0, 49, 4_900), 0);
        assert_eq!(compute_score(0, 49, 5_000), 100);
    }

    #[test]
    fn compute_score_soft_50_fail() {
        // 50 件到達で FAIL = 0
        assert_eq!(compute_score(0, 50, 1_000_000), 0);
        assert_eq!(compute_score(0, 51, 1_000_000), 0);
    }

    #[test]
    fn compute_score_critical_fail() {
        // critical>=1 は即時 FAIL
        assert_eq!(compute_score(1, 0, 1_000_000), 0);
        assert_eq!(compute_score(2, 0, 1_000_000), 0);
    }

    #[test]
    fn compute_score_saturating_does_not_panic() {
        // 理論上の overflow が saturating で抑えられることを確認 (49 未満でも fail でもない経路)
        let _ = compute_score(0, SOFT_FAIL_THRESHOLD - 1, i64::MAX);
    }

    #[tokio::test]
    async fn map_reqwest_classifies_timeout() {
        use axum::routing::get;
        let app = axum::Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(3)).await;
                "ok"
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let state = BenchState::new(Vec::new(), 0);
        let client = Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .unwrap();

        let url = format!("http://{addr}/slow");
        let err = client
            .get(&url)
            .send()
            .await
            .expect_err("expected timeout error");
        assert!(err.is_timeout(), "expected is_timeout, got: {err}");

        let mapped = map_reqwest(&state, "test get", err);
        match mapped {
            BenchError::Soft(ctx) => assert_eq!(ctx, "test get"),
            BenchError::Other(s) => panic!("expected Soft, got Other: {s}"),
        }
        assert_eq!(state.soft_count.load(Ordering::Relaxed), 1);

        server.abort();
    }

    #[tokio::test]
    async fn map_reqwest_classifies_decode_error_as_soft() {
        // 200 OK + 非 JSON body → resp.json() で parse error (= manual.md §6.5
        // 「レスポンス形式不正」= soft 計上対象)
        use axum::routing::get;
        let app = axum::Router::new().route("/text", get(|| async { "not-json-body" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let state = BenchState::new(Vec::new(), 0);
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();

        let url = format!("http://{addr}/text");
        let resp = client.get(&url).send().await.unwrap();
        let err = resp
            .json::<i32>()
            .await
            .expect_err("expected json parse error");
        assert!(!err.is_timeout(), "expected non-timeout, got: {err}");
        assert!(err.is_decode(), "expected decode error, got: {err}");

        let mapped = map_reqwest(&state, "test json", err);
        match mapped {
            BenchError::Soft(ctx) => assert_eq!(ctx, "test json"),
            BenchError::Other(s) => panic!("expected Soft, got Other: {s}"),
        }
        // decode err は §6.5 に従い soft 1 件計上
        assert_eq!(state.soft_count.load(Ordering::Relaxed), 1);

        server.abort();
    }

    #[tokio::test]
    async fn map_status_counts_soft_and_returns_soft_variant() {
        // 500 を返すサーバ → error_for_status で reqwest::Error → map_status で soft 計上
        use axum::routing::get;
        let app = axum::Router::new().route(
            "/err",
            get(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let state = BenchState::new(Vec::new(), 0);
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();

        let url = format!("http://{addr}/err");
        let resp = client.get(&url).send().await.unwrap();
        let err = resp
            .error_for_status()
            .expect_err("expected status error");

        let mapped = map_status(&state, "test status", err);
        match mapped {
            BenchError::Soft(ctx) => assert_eq!(ctx, "test status"),
            BenchError::Other(s) => panic!("expected Soft, got Other: {s}"),
        }
        assert_eq!(state.soft_count.load(Ordering::Relaxed), 1);

        server.abort();
    }

    #[test]
    fn add_soft_counts_and_cancels_at_threshold() {
        let state = BenchState::new(Vec::new(), 0);
        // 49 件目までは cancel されない
        for _ in 0..(SOFT_FAIL_THRESHOLD - 1) {
            state.add_soft("probe");
        }
        assert_eq!(state.soft_count.load(Ordering::Relaxed), 49);
        assert!(!state.cancel_global.is_cancelled());

        // 50 件目で cancel 発火
        let count = state.add_soft("probe");
        assert_eq!(count, 50);
        assert_eq!(state.soft_count.load(Ordering::Relaxed), 50);
        assert!(state.cancel_global.is_cancelled());

        // 51 件目以降は cancel 状態維持 (cancel は冪等)
        let count = state.add_soft("probe");
        assert_eq!(count, 51);
        assert!(state.cancel_global.is_cancelled());
    }

    /// テスト用 closed snap helper。idea.md 仕様で closed なら
    /// current_count == goal_count == participants.len。
    fn make_closed_snap(id: &str, goal: i32) -> CampaignSnap {
        let participants = (0..goal as usize)
            .map(|i| ParticipantSnap {
                user_id: format!("user-{i}"),
                name: format!("name-{i}"),
                joined_at: "2026-01-01T00:00:00.000Z".to_string(),
            })
            .collect::<Vec<_>>();
        CampaignSnap {
            id: id.to_string(),
            name: format!("camp-{id}"),
            description: String::new(),
            price: 1000,
            goal_count: goal,
            current_count: goal,
            tags: Vec::new(),
            status: "closed".to_string(),
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
            last_joined_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            participants,
        }
    }

    #[test]
    fn validation_close_does_not_emit_seller_event() {
        // Phase::Validation の close は seller queue に流れない (integrity_scenario 等の保護)。
        let state = BenchState::new(Vec::new(), 0);
        let (tx, mut rx) = mpsc::channel::<()>(8);
        state.install_load_close_channel(tx);

        let snap = make_closed_snap("c1", 2);
        state.record_campaign_observation(&snap, Phase::Validation);

        assert!(rx.try_recv().is_err(), "validation close must not emit");
        assert_eq!(state.seller_events_emitted.load(Ordering::Relaxed), 0);
        assert_eq!(state.seller_queue_dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn load_close_emits_event_once_per_campaign() {
        // 同じ closed snap を 2 回観測しても event は 1 回 (closed_campaigns dedup の帰結)。
        let state = BenchState::new(Vec::new(), 0);
        let (tx, mut rx) = mpsc::channel::<()>(8);
        state.install_load_close_channel(tx);

        let snap = make_closed_snap("c1", 2);
        state.record_campaign_observation(&snap, Phase::Load);
        state.record_campaign_observation(&snap, Phase::Load);

        assert!(rx.try_recv().is_ok(), "first load close must emit");
        assert!(
            rx.try_recv().is_err(),
            "second observation must not emit (dedup)"
        );
        assert_eq!(state.seller_events_emitted.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn load_close_without_channel_does_not_panic() {
        // run_load_phase 開始前 / 終了後の close 観測でも notify_load_close が安全に no-op。
        let state = BenchState::new(Vec::new(), 0);
        let snap = make_closed_snap("c1", 2);
        state.record_campaign_observation(&snap, Phase::Load);
        assert_eq!(state.seller_events_emitted.load(Ordering::Relaxed), 0);
        assert_eq!(state.seller_queue_dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn load_close_full_queue_increments_dropped() {
        // bounded queue が fill 中の close event は seller_queue_dropped に積まれる
        // (Full と Closed の混同回避をテストレベルで焼く)。
        let state = BenchState::new(Vec::new(), 0);
        let (tx, _rx) = mpsc::channel::<()>(1);
        state.install_load_close_channel(tx);

        // 1 件目: try_send 成功 → emitted++
        state.record_campaign_observation(&make_closed_snap("c1", 2), Phase::Load);
        // 2 件目以降: queue 満杯 (_rx は drain しない) → dropped++
        state.record_campaign_observation(&make_closed_snap("c2", 2), Phase::Load);
        state.record_campaign_observation(&make_closed_snap("c3", 2), Phase::Load);

        assert_eq!(state.seller_events_emitted.load(Ordering::Relaxed), 1);
        assert_eq!(state.seller_queue_dropped.load(Ordering::Relaxed), 2);
        assert_eq!(state.seller_channel_closed.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn load_close_after_receiver_drop_increments_channel_closed() {
        // receiver 喪失 (seller actor の create_user 失敗等) 後の close event は
        // seller_channel_closed に積まれる (= Full とは別カウンタ)。
        let state = BenchState::new(Vec::new(), 0);
        let (tx, rx) = mpsc::channel::<()>(8);
        state.install_load_close_channel(tx);
        drop(rx); // receiver を即時 drop = TrySendError::Closed 経路

        state.record_campaign_observation(&make_closed_snap("c1", 2), Phase::Load);
        state.record_campaign_observation(&make_closed_snap("c2", 2), Phase::Load);

        assert_eq!(state.seller_events_emitted.load(Ordering::Relaxed), 0);
        assert_eq!(state.seller_queue_dropped.load(Ordering::Relaxed), 0);
        assert_eq!(state.seller_channel_closed.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn close_load_close_channel_makes_emits_no_op() {
        // close_load_close_channel() で BenchState から sender が外れたら、
        // notify_load_close は no-op (= installed sender が無い分岐)。
        // sender clone の race を保証するわけではない (best-effort)。
        let state = BenchState::new(Vec::new(), 0);
        let (tx, mut rx) = mpsc::channel::<()>(8);
        state.install_load_close_channel(tx);

        state.record_campaign_observation(&make_closed_snap("c1", 2), Phase::Load);
        state.close_load_close_channel();
        state.record_campaign_observation(&make_closed_snap("c2", 2), Phase::Load);

        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
        assert_eq!(state.seller_events_emitted.load(Ordering::Relaxed), 1);
        assert_eq!(state.seller_channel_closed.load(Ordering::Relaxed), 0);
    }
}
