//! 配布版 seed.sql / dev fallback seed.base.sql を生成する作問ツール。
//!
//! 配布物には含めない (CLAUDE.md「Everything else (build pipeline, etc.)」)。
//! seed-data crate を single source of truth として、UUID 定数 / tag / base
//! campaigns / participants をそこから引き、SQL に書き出す。
//!
//! usage:
//!
//!   seed-gen base --out FILE
//!     dev fallback 用の seed.base.sql を生成 (画像はダミー JPEG magic 6 byte)。
//!     UUID 定数を変えたら必ず再実行して webapp/sql/seed.base.sql を commit する。
//!
//!   seed-gen full --count N --seed S --fixtures DIR --out FILE
//!     配布版 seed.sql を生成。base 内容 + N 件の generated campaigns + 実画像。
//!     scripts/build.sh から呼ばれる。
//!
//! base / full どちらも seed-data 定数のみから完全レンダリングし、現行の
//! 「seed.base.sql を読んで concat する」経路は使わない (= UUID ローテ中の
//! 中間状態で base 旧 UUID と generated 新 UUID が混ざる事故を構造的に防ぐ)。

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use seed_data::{
    SeedCampaign, BASE_CAMPAIGNS, BASE_PARTICIPANTS, BASE_USERS, GENERATED_CAMPAIGN_COUNT, PRICES,
    TAGS,
};
use std::collections::HashMap;
use std::fs;
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// MySQL の max_allowed_packet を視野に、1 statement あたり 8 MiB を上限とする。
const MAX_STMT_BYTES: usize = 8 * 1024 * 1024;

/// MySQL string literal を組む。`'` を `''` にエスケープする。
/// (現行 seed-data には `'` を含む name はないが、将来 opaque pool / 任意 name を
/// 入れた時の事故を防ぐ。)
fn sql_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// base mode で全 campaign に注入する dummy 画像。NOT NULL 制約を満たすだけの
/// 最小 JPEG magic (FF D8 FF E0 00 10)。bench は base campaign の image_hash を
/// 検証しないため正規 JPEG である必要はない。
const BASE_DUMMY_IMAGE: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];

// ===== CLI =====

enum Command {
    Base {
        out: PathBuf,
    },
    Full {
        count: usize,
        seed: u64,
        fixtures: PathBuf,
        out: PathBuf,
    },
}

const USAGE: &str = "usage:
  seed-gen base --out FILE
  seed-gen full --count N --seed S --fixtures DIR --out FILE";

fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("{}\n{}", msg.as_ref(), USAGE);
    std::process::exit(1);
}

fn parse_args() -> Command {
    let argv: Vec<String> = std::env::args().collect();
    let sub = match argv.get(1) {
        Some(s) => s.as_str(),
        None => die("missing subcommand"),
    };
    if matches!(sub, "-h" | "--help" | "help") {
        eprintln!("{}", USAGE);
        std::process::exit(0);
    }

    // remaining args は --key value のペアで受ける (簡易 parser)。
    let rest = &argv[2..];
    let mut kv: HashMap<String, String> = HashMap::new();
    let mut i = 0;
    while i < rest.len() {
        let key = &rest[i];
        if !key.starts_with("--") {
            die(format!("expected --flag, got {:?}", key));
        }
        let val = match rest.get(i + 1) {
            Some(v) => v.clone(),
            None => die(format!("flag {:?} requires a value", key)),
        };
        if kv.insert(key.clone(), val).is_some() {
            die(format!("duplicate flag {:?}", key));
        }
        i += 2;
    }
    let mut take = |k: &str| -> String {
        kv.remove(k)
            .unwrap_or_else(|| die(format!("missing required flag {}", k)))
    };
    let cmd = match sub {
        "base" => Command::Base {
            out: PathBuf::from(take("--out")),
        },
        "full" => {
            let count: usize = take("--count")
                .parse()
                .unwrap_or_else(|e| die(format!("--count: {e}")));
            if count > GENERATED_CAMPAIGN_COUNT {
                die(format!(
                    "--count {} exceeds GENERATED_CAMPAIGN_COUNT {} (= seed-data の上限)",
                    count, GENERATED_CAMPAIGN_COUNT
                ));
            }
            let seed_s = take("--seed");
            let seed: u64 = if let Some(hex) = seed_s.strip_prefix("0x") {
                u64::from_str_radix(hex, 16)
                    .unwrap_or_else(|e| die(format!("--seed hex: {e}")))
            } else {
                seed_s
                    .parse()
                    .unwrap_or_else(|e| die(format!("--seed integer: {e}")))
            };
            Command::Full {
                count,
                seed,
                fixtures: PathBuf::from(take("--fixtures")),
                out: PathBuf::from(take("--out")),
            }
        }
        other => die(format!("unknown subcommand {:?}", other)),
    };
    if !kv.is_empty() {
        die(format!(
            "unrecognized flags: {:?}",
            kv.keys().collect::<Vec<_>>()
        ));
    }
    cmd
}

fn main() {
    match parse_args() {
        Command::Base { out } => emit_base(&out),
        Command::Full {
            count,
            seed,
            fixtures,
            out,
        } => emit_full(count, seed, &fixtures, &out),
    }
}

// ===== chunked INSERT writer =====

/// 多行 INSERT を 8 MiB / statement で chunk しながらストリーミング書き出しする。
struct ChunkedInsert<'a, W: std::io::Write> {
    w: &'a mut W,
    header: &'a str,
    buf: String,
    in_stmt: bool,
}

impl<'a, W: std::io::Write> ChunkedInsert<'a, W> {
    fn new(w: &'a mut W, header: &'a str) -> Self {
        Self {
            w,
            header,
            buf: String::with_capacity(MAX_STMT_BYTES + 1024 * 1024),
            in_stmt: false,
        }
    }

    /// 1 行を append。row は `(...)` 形式の値タプル文字列 (前の "  " インデント
    /// と後ろの "," は writer 側で付けるので含めない)。
    fn push_row(&mut self, row: &str) {
        // 既存 statement に追加するなら ",\n" + "  " + row + ";\n" 終端を見込む。
        // 新 statement なら header + "  " + row + ";\n" を見込む。
        let projected = self.buf.len()
            + if self.in_stmt { 2 } else { self.header.len() }
            + 2
            + row.len()
            + 2;
        if self.in_stmt && projected > MAX_STMT_BYTES {
            self.flush();
        }
        if !self.in_stmt {
            self.buf.push_str(self.header);
            self.in_stmt = true;
        } else {
            self.buf.push_str(",\n");
        }
        self.buf.push_str("  ");
        self.buf.push_str(row);
    }

    fn flush(&mut self) {
        if self.in_stmt {
            self.buf.push_str(";\n");
            self.w.write_all(self.buf.as_bytes()).expect("write SQL");
            self.buf.clear();
            self.in_stmt = false;
        }
    }
}

// ===== emit (base / full 共通の section) =====

const USERS_HEADER: &str =
    "INSERT INTO `users` (`id`, `name`, `credit_limit`, `created_at`) VALUES\n";
const TAGS_HEADER: &str = "INSERT INTO `tags` (`id`, `name`, `created_at`) VALUES\n";
const CAMPAIGNS_HEADER: &str = "INSERT INTO `campaigns` (`id`, `name`, `description`, `price`, `goal_count`, `image`, `created_at`) VALUES\n";
const CAMPAIGN_TAGS_HEADER: &str =
    "INSERT INTO `campaign_tags` (`campaign_id`, `tag_id`, `created_at`) VALUES\n";
const PARTICIPANTS_HEADER: &str =
    "INSERT INTO `campaign_participants` (`id`, `campaign_id`, `user_id`, `created_at`) VALUES\n";
const APP_CONFIG_HEADER: &str = "INSERT INTO `app_config` (`name`, `value`) VALUES\n";

/// tags は seed が壊れるとすぐ気付くように tag 名でも seed_data 側 const と
/// 一致するか軽く assert する用の created_at。tags の created_at は seed.base.sql
/// では '2026-01-01 09:00:00.000000' で固定。
const TAGS_CREATED_AT: &str = "2026-01-01 09:00:00.000000";

fn write_users<W: std::io::Write>(w: &mut W) {
    let mut ci = ChunkedInsert::new(w, USERS_HEADER);
    for u in BASE_USERS {
        let row = format!(
            "('{}', {}, {}, {})",
            u.id,
            sql_str(u.name),
            u.credit_limit,
            sql_str(u.created_at)
        );
        ci.push_row(&row);
    }
    ci.flush();
}

fn write_tags<W: std::io::Write>(w: &mut W) {
    let mut ci = ChunkedInsert::new(w, TAGS_HEADER);
    for t in TAGS {
        let row = format!(
            "('{}', {}, {})",
            t.id,
            sql_str(t.name),
            sql_str(TAGS_CREATED_AT)
        );
        ci.push_row(&row);
    }
    ci.flush();
}

fn write_base_participants<W: std::io::Write>(w: &mut W) {
    let mut ci = ChunkedInsert::new(w, PARTICIPANTS_HEADER);
    for p in BASE_PARTICIPANTS {
        let row = format!(
            "('{}', '{}', '{}', {})",
            p.id,
            p.campaign_id,
            p.user_id,
            sql_str(p.created_at)
        );
        ci.push_row(&row);
    }
    ci.flush();
}

fn write_app_config<W: std::io::Write>(w: &mut W) {
    // initialize 時に UPSERT で上書きされるので空文字を 1 行入れるだけ。
    writeln!(w, "{}  ('notification_webhook_url', '');", APP_CONFIG_HEADER).expect("write");
}

fn campaign_row_sql(c: &SeedCampaign, image_hex: &str) -> String {
    format!(
        "('{}', {}, {}, {}, {}, x'{}', {})",
        c.id,
        sql_str(c.name),
        sql_str(c.description),
        c.price,
        c.goal_count,
        image_hex,
        sql_str(c.created_at)
    )
}

/// (campaign_id, tag_id, created_at)
type TagRow = (Uuid, Uuid, String);

fn append_base_campaign_tag_rows(out: &mut Vec<TagRow>) {
    for c in BASE_CAMPAIGNS {
        for tid in c.tag_ids {
            out.push((c.id, *tid, c.created_at.to_string()));
        }
    }
}

fn write_campaign_tag_rows<W: std::io::Write>(w: &mut W, rows: &[TagRow]) {
    let mut ci = ChunkedInsert::new(w, CAMPAIGN_TAGS_HEADER);
    for (cid, tid, ca) in rows {
        let row = format!("('{}', '{}', {})", cid, tid, sql_str(ca));
        ci.push_row(&row);
    }
    ci.flush();
}

// ===== base mode =====

fn emit_base(out: &Path) {
    let f = fs::File::create(out).unwrap_or_else(|e| die(format!("create {:?}: {e}", out)));
    let mut w = BufWriter::new(f);

    writeln!(
        w,
        "-- nrb2026 webapp seed (base, dev fallback)。seed-gen が seed-data から自動生成する。\n\
         -- 手で編集しないこと。UUID 定数を変えたら `cargo run -p seed-gen -- base --out webapp/sql/seed.base.sql` で再生成して commit。\n\
         -- 配布版 seed としては seed-gen full モードが seed.sql (1500 件 + 画像) を生成する。\n\
         -- このファイルは seed.sql が無いとき (= dev fresh checkout) の fallback。\n"
    )
    .expect("write header");

    write_users(&mut w);
    write_tags(&mut w);

    // campaigns: 画像は dummy magic 固定
    {
        let mut ci = ChunkedInsert::new(&mut w, CAMPAIGNS_HEADER);
        let img_hex = hex::encode_upper(BASE_DUMMY_IMAGE);
        for c in BASE_CAMPAIGNS {
            ci.push_row(&campaign_row_sql(c, &img_hex));
        }
        ci.flush();
    }

    // campaign_tags
    let mut tag_rows: Vec<TagRow> = Vec::new();
    append_base_campaign_tag_rows(&mut tag_rows);
    write_campaign_tag_rows(&mut w, &tag_rows);

    write_base_participants(&mut w);
    write_app_config(&mut w);

    w.flush().expect("flush");
    eprintln!(
        "seed-gen: wrote base SQL to {:?} ({} users / {} tags / {} campaigns / {} campaign_tags / {} participants)",
        out,
        BASE_USERS.len(),
        TAGS.len(),
        BASE_CAMPAIGNS.len(),
        tag_rows.len(),
        BASE_PARTICIPANTS.len(),
    );
}

// ===== full mode =====

fn load_fixtures(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| die(format!("read fixtures dir {:?}: {e}", dir)))
        .filter_map(|r| r.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jpg"))
        .collect();
    entries.sort();
    if entries.is_empty() {
        die(format!("no .jpg fixtures in {:?}", dir));
    }
    eprintln!("seed-gen: {} fixture jpgs in {:?}", entries.len(), dir);
    entries
}

fn emit_full(count: usize, seed: u64, fixtures_dir: &Path, out: &Path) {
    let fixtures = load_fixtures(fixtures_dir);
    let mut rng = StdRng::seed_from_u64(seed);

    let f = fs::File::create(out).unwrap_or_else(|e| die(format!("create {:?}: {e}", out)));
    let mut w = BufWriter::new(f);

    writeln!(
        w,
        "-- nrb2026 webapp seed (full, distributed)。seed-gen が seed-data + fixture jpgs から自動生成する。\n\
         -- 手で編集しないこと。scripts/build.sh が AMI ビルド時に再生成する。\n\
         -- 設定: seed-gen full --count={} --seed=0x{:x}\n",
        count, seed
    )
    .expect("write header");

    writeln!(w, "SET autocommit=0;").expect("write");
    writeln!(w).expect("write");

    write_users(&mut w);
    write_tags(&mut w);

    // base + generated campaigns を 1 つの chunked INSERT にまとめて流す。
    // 同じ pass で generated 用の tag rows も収集しておく (rng 消費順を維持
    // するため)。base 部分の画像は dummy 固定、generated は fixture から random pick。
    let mut tag_rows: Vec<TagRow> = Vec::new();
    append_base_campaign_tag_rows(&mut tag_rows);

    {
        let mut ci = ChunkedInsert::new(&mut w, CAMPAIGNS_HEADER);

        // base campaigns (画像は dummy)
        let dummy_hex = hex::encode_upper(BASE_DUMMY_IMAGE);
        for c in BASE_CAMPAIGNS {
            ci.push_row(&campaign_row_sql(c, &dummy_hex));
        }

        // generated campaigns
        for n in 0..count {
            let id = seed_data::generated_campaign_id(n);
            let price = PRICES[n % PRICES.len()];
            let goal_count = 2 + (n % 19) as i32;
            // created_at: 現行 seed_gen と同じ式 (n から日時を組み立て)
            let total_sec = n;
            let h = (total_sec / 3600) % 24;
            let mins = (total_sec / 60) % 60;
            let s = total_sec % 60;
            let day = 1 + total_sec / 86400;
            let created_at =
                format!("2026-01-{:02} {:02}:{:02}:{:02}.000000", day, h, mins, s);

            // rng 消費 #1: fixture pick
            let img_path = fixtures.choose(&mut rng).expect("non-empty fixtures");
            let img_bytes = fs::read(img_path)
                .unwrap_or_else(|e| die(format!("read fixture {:?}: {e}", img_path)));
            let img_hex = hex::encode_upper(&img_bytes);

            let row = format!(
                "('{}', 'テスト椅子 No.{}', 'seed_gen で自動生成したテスト用 椅子 No.{} の説明', {}, {}, x'{}', '{}')",
                id, n, n, price, goal_count, img_hex, created_at
            );
            ci.push_row(&row);

            // rng 消費 #2..: tag picks (1〜3 個、重複 silently skip)
            let n_tags = 1 + (n % 3);
            let mut picked: Vec<Uuid> = Vec::with_capacity(n_tags);
            for _ in 0..n_tags {
                let t = TAGS.choose(&mut rng).expect("non-empty TAGS");
                if !picked.contains(&t.id) {
                    picked.push(t.id);
                    tag_rows.push((id, t.id, created_at.clone()));
                }
            }
        }
        ci.flush();
    }

    write_campaign_tag_rows(&mut w, &tag_rows);
    write_base_participants(&mut w);
    write_app_config(&mut w);

    writeln!(w).expect("write");
    writeln!(w, "COMMIT;").expect("write");
    w.flush().expect("flush");

    eprintln!(
        "seed-gen: wrote full SQL to {:?} ({} base + {} generated campaigns / {} campaign_tags rows)",
        out,
        BASE_CAMPAIGNS.len(),
        count,
        tag_rows.len(),
    );
}
