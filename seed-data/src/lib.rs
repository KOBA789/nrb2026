//! nrb2026 seed 定数 (作問用)。
//!
//! 配布版 webapp / bench / seed-gen の 3 者が参照する seed 定数の
//! single source of truth。
//!
//! - `webapp` は seed-data に依存しない (= 配布物として self-contained)。
//!   webapp が seed を読み込むのは /api/initialize で seed.sql /
//!   seed.base.sql ファイルを mysql client 経由で流すときだけ。
//! - `seed-gen` は seed-data 定数を SQL に書き出す。
//! - `bench` は seed-data から seed の UUID / tag 名 / 件数を引いて、
//!   pretest / 整合性検査の固定参照に使う。
//!
//! UUID と tag 値はこの crate を変更したら必ず:
//!   1. `cargo test -p seed-data` (uniqueness の unit test) を pass させる
//!   2. `cargo run --release -p seed-gen -- base --out webapp/sql/seed.base.sql`
//!      で fallback seed を再生成して commit する
//!      (`scripts/e2e.sh` は冒頭で `webapp/sql/seed.sql` を削除して base.sql に
//!      フォールバックするので、e2e 経路では古い配布版 seed が残る事故は起きない)
//!   3. `bench` の hardcode は seed-data 経由のみ (UUID / tag 名を直接書かない)
//!
//! UUID はランダム v4 で固定。連番 / パターン化された UUID は使わない
//! (生成 campaign の予測可能性回避と、参考実装の見栄え)。
//! generated campaigns は UUID v5 + 固定 namespace で deterministic に導出する。

use uuid::{uuid, Uuid};

#[derive(Debug, Clone, Copy)]
pub struct Tag {
    pub id: Uuid,
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct SeedUser {
    pub id: Uuid,
    pub name: &'static str,
    pub credit_limit: i32,
    pub created_at: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct SeedCampaign {
    pub id: Uuid,
    pub name: &'static str,
    pub description: &'static str,
    pub price: i32,
    pub goal_count: i32,
    pub created_at: &'static str,
    pub tag_ids: &'static [Uuid],
}

#[derive(Debug, Clone, Copy)]
pub struct SeedParticipant {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub user_id: Uuid,
    pub created_at: &'static str,
}

// ===== tags (20) =====
pub const TAGS: &[Tag] = &[
    Tag { id: uuid!("6ae98125-52bc-4132-af51-d4509de5d2d7"), name: "comfortable" },
    Tag { id: uuid!("5eec0c63-65fb-4bea-8e34-252e82666345"), name: "ergonomic"   },
    Tag { id: uuid!("b9bfb9ab-7f84-43a2-af49-0c77e545e8d4"), name: "mesh"        },
    Tag { id: uuid!("10e978e9-bede-4929-9bcf-5d1e7cba5b96"), name: "leather"     },
    Tag { id: uuid!("ebdd3a6f-3024-4d53-baa4-bbff206d923a"), name: "gaming"      },
    Tag { id: uuid!("e7d4cade-00d1-41d0-86e4-32b0e0300985"), name: "office"      },
    Tag { id: uuid!("14ae3d63-9e59-4bdd-961e-b8998b293358"), name: "dining"      },
    Tag { id: uuid!("daaaae99-25e2-47a9-83fa-29b124b95b0b"), name: "lounge"      },
    Tag { id: uuid!("69600538-4e18-4545-9063-f459f9985753"), name: "folding"     },
    Tag { id: uuid!("b0babbf0-f1ed-4b92-b3b7-3e7a16939755"), name: "swivel"      },
    Tag { id: uuid!("707e5d18-7039-4c90-b263-2f90b9019657"), name: "recliner"    },
    Tag { id: uuid!("f42b8388-f1c7-474b-a48b-ee94073e1841"), name: "wooden"      },
    Tag { id: uuid!("5da7df14-3b6f-4930-b3e9-389a0478970c"), name: "metal"       },
    Tag { id: uuid!("a8725933-13b2-4624-acde-bd2f3bf8773d"), name: "plastic"     },
    Tag { id: uuid!("761ac618-f9a1-4c32-9585-e434879a8419"), name: "fabric"      },
    Tag { id: uuid!("ff7d8eec-abce-4c9e-b348-201ab13b378f"), name: "armrest"     },
    Tag { id: uuid!("a7223c4f-b87d-4ab6-a841-0e2fcec090a8"), name: "headrest"    },
    Tag { id: uuid!("83fc9039-ad85-4d89-b13b-746dfad71a67"), name: "lumbar"      },
    Tag { id: uuid!("4630f3b1-5a40-4ddd-a8bf-5cf922d2e79d"), name: "tall"        },
    Tag { id: uuid!("e60fb244-101e-4c4d-acb8-e711642ca770"), name: "compact"     },
];

// ===== users (5) =====
pub const BASE_USERS: &[SeedUser] = &[
    SeedUser { id: uuid!("f10a3eef-ff69-4cc5-a098-f225321d52e5"), name: "alice", credit_limit: 60000, created_at: "2026-01-01 10:00:00.000000" },
    SeedUser { id: uuid!("f6d35d84-9808-4ae3-ab95-ab4aa953e2be"), name: "bob",   credit_limit: 60000, created_at: "2026-01-01 10:00:01.000000" },
    SeedUser { id: uuid!("4e4a5313-8bf2-4298-bf08-05656ebbafbb"), name: "carol", credit_limit: 60000, created_at: "2026-01-01 10:00:02.000000" },
    SeedUser { id: uuid!("a38e13b1-591b-4497-9c72-ff45e23d63ef"), name: "dave",  credit_limit: 60000, created_at: "2026-01-01 10:00:03.000000" },
    SeedUser { id: uuid!("ba66b58a-2621-4db8-ba00-f60c5cb9b6d4"), name: "eve",   credit_limit: 60000, created_at: "2026-01-01 10:00:04.000000" },
];

// ===== campaigns (5, 全部 open / charges 無し) =====
pub const BASE_CAMPAIGNS: &[SeedCampaign] = &[
    SeedCampaign {
        id: uuid!("100b73b1-c334-4231-89c2-0bca6ad9da55"),
        name: "メッシュなオフィスチェア",
        description: "通気性抜群のメッシュ素材で長時間座っても蒸れにくい",
        price: 15000,
        goal_count: 5,
        created_at: "2026-04-01 12:00:00.000000",
        tag_ids: &[
            uuid!("b9bfb9ab-7f84-43a2-af49-0c77e545e8d4"), // mesh
            uuid!("5eec0c63-65fb-4bea-8e34-252e82666345"), // ergonomic
            uuid!("e7d4cade-00d1-41d0-86e4-32b0e0300985"), // office
        ],
    },
    SeedCampaign {
        id: uuid!("db734fd4-9c39-4e0a-82b5-a5ae92b1028d"),
        name: "ゲーミングチェア",
        description: "高めヘッドレストとランバーサポート付き、長時間ゲーム向け",
        price: 18000,
        goal_count: 4,
        created_at: "2026-04-02 12:00:00.000000",
        tag_ids: &[
            uuid!("ebdd3a6f-3024-4d53-baa4-bbff206d923a"), // gaming
            uuid!("a7223c4f-b87d-4ab6-a841-0e2fcec090a8"), // headrest
            uuid!("83fc9039-ad85-4d89-b13b-746dfad71a67"), // lumbar
        ],
    },
    SeedCampaign {
        id: uuid!("9c42df95-67b7-4dca-b3d4-0913c3491357"),
        name: "レザーリクライナー",
        description: "本革張り、フルリクライニング、リビングに置ける高級椅子",
        price: 20000,
        goal_count: 3,
        created_at: "2026-04-03 12:00:00.000000",
        tag_ids: &[
            uuid!("10e978e9-bede-4929-9bcf-5d1e7cba5b96"), // leather
            uuid!("707e5d18-7039-4c90-b263-2f90b9019657"), // recliner
            uuid!("daaaae99-25e2-47a9-83fa-29b124b95b0b"), // lounge
        ],
    },
    SeedCampaign {
        id: uuid!("3cec9103-f883-4316-a96e-d099517de0a2"),
        name: "折りたたみ椅子3脚セット",
        description: "来客用に。コンパクトで収納場所を取らない",
        price: 8000,
        goal_count: 10,
        created_at: "2026-04-04 12:00:00.000000",
        tag_ids: &[
            uuid!("69600538-4e18-4545-9063-f459f9985753"), // folding
            uuid!("e60fb244-101e-4c4d-acb8-e711642ca770"), // compact
        ],
    },
    SeedCampaign {
        id: uuid!("91f02ead-1dd7-4352-85ed-0f2959bb0e71"),
        name: "木製ダイニングチェア",
        description: "ナチュラルな木目、家族で囲めるテーブル用ダイニングチェア",
        price: 12000,
        goal_count: 6,
        created_at: "2026-04-05 12:00:00.000000",
        tag_ids: &[
            uuid!("f42b8388-f1c7-474b-a48b-ee94073e1841"), // wooden
            uuid!("14ae3d63-9e59-4bdd-961e-b8998b293358"), // dining
        ],
    },
];

// ===== campaign_participants (5) =====
//
// C1: alice + bob, C2: carol, C3/C4: 0 名, C5: dave + eve
pub const BASE_PARTICIPANTS: &[SeedParticipant] = &[
    SeedParticipant {
        id: uuid!("b26449dd-23d6-4437-bfc3-f65daf0b3dca"),
        campaign_id: uuid!("100b73b1-c334-4231-89c2-0bca6ad9da55"),
        user_id: uuid!("f10a3eef-ff69-4cc5-a098-f225321d52e5"),
        created_at: "2026-04-10 10:00:00.000000",
    },
    SeedParticipant {
        id: uuid!("ee95ac74-ed0f-49f1-809e-fe8e4e61186a"),
        campaign_id: uuid!("100b73b1-c334-4231-89c2-0bca6ad9da55"),
        user_id: uuid!("f6d35d84-9808-4ae3-ab95-ab4aa953e2be"),
        created_at: "2026-04-10 11:00:00.000000",
    },
    SeedParticipant {
        id: uuid!("7b524c35-43c4-4b81-b10f-25c06d3ed80d"),
        campaign_id: uuid!("db734fd4-9c39-4e0a-82b5-a5ae92b1028d"),
        user_id: uuid!("4e4a5313-8bf2-4298-bf08-05656ebbafbb"),
        created_at: "2026-04-11 10:00:00.000000",
    },
    SeedParticipant {
        id: uuid!("456a7a08-9bfc-4614-bf85-faf2eb185eea"),
        campaign_id: uuid!("91f02ead-1dd7-4352-85ed-0f2959bb0e71"),
        user_id: uuid!("a38e13b1-591b-4497-9c72-ff45e23d63ef"),
        created_at: "2026-04-13 10:00:00.000000",
    },
    SeedParticipant {
        id: uuid!("f55658ea-dc08-49b9-bb85-a29806dbc493"),
        campaign_id: uuid!("91f02ead-1dd7-4352-85ed-0f2959bb0e71"),
        user_id: uuid!("ba66b58a-2621-4db8-ba00-f60c5cb9b6d4"),
        created_at: "2026-04-13 11:00:00.000000",
    },
];

// ===== price 分布 (seed-gen 拡張部で modulo 使用) =====
pub const PRICES: &[i32] = &[2000, 3000, 4000, 5000, 7000, 10000, 12000, 15000, 18000, 20000];

// ===== bench から名前で参照する tag 群 =====
//
// bench の負荷シナリオ / pretest が直接参照する tag 名の named const。
// API は tag を名前で扱うので UUID は seed-gen 経由でしか必要にならない。
pub mod tag {
    pub const MESH: &str = "mesh";
    pub const ERGONOMIC: &str = "ergonomic";
}

/// bench の pretest が `/api/tags` レスポンスの中に必ず含まれるべき tag。
/// seed が破損した場合に「seed が壊れている」とすぐ分かるようにする
/// (= bench の create_campaign が 400 にすり替わるのを回避)。
pub const REQUIRED_TAG_NAMES: &[&str] = &[tag::MESH, tag::ERGONOMIC];

// ===== generated campaign 群 (seed-gen full モードが 1500 件) =====
pub const GENERATED_CAMPAIGN_COUNT: usize = 1500;

/// generated campaign の UUID v5 namespace。固定値。
pub const GENERATED_CAMPAIGN_NAMESPACE: Uuid = uuid!("5eddfc78-b9d7-409c-913e-5e2d3162173b");

/// n 番目 generated campaign の deterministic UUID。
///
/// UUID v5 (固定 namespace + index の 16 byte name)。bench から
/// 「n 番目を狙う」シナリオで O(1) 参照、衝突確率は 1500 件で実質 0。
/// uniqueness 検査は `tests::all_seed_uuids_are_unique` で全列挙して assert。
pub fn generated_campaign_id(n: usize) -> Uuid {
    assert!(
        n < GENERATED_CAMPAIGN_COUNT,
        "generated_campaign_id: n={} out of range (max {})",
        n,
        GENERATED_CAMPAIGN_COUNT
    );
    // name = b"campaign" (8 byte) + (n as u64 BE) (8 byte) = 16 byte
    let mut name = [0u8; 16];
    name[..8].copy_from_slice(b"campaign");
    name[8..].copy_from_slice(&(n as u64).to_be_bytes());
    Uuid::new_v5(&GENERATED_CAMPAIGN_NAMESPACE, &name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_seed_uuids_are_unique() {
        let mut s = HashSet::new();
        for t in TAGS {
            assert!(s.insert(t.id), "duplicate tag id {}", t.id);
        }
        for u in BASE_USERS {
            assert!(s.insert(u.id), "duplicate user id {}", u.id);
        }
        for c in BASE_CAMPAIGNS {
            assert!(s.insert(c.id), "duplicate campaign id {}", c.id);
        }
        for p in BASE_PARTICIPANTS {
            assert!(s.insert(p.id), "duplicate participant id {}", p.id);
        }
        for n in 0..GENERATED_CAMPAIGN_COUNT {
            let id = generated_campaign_id(n);
            assert!(s.insert(id), "duplicate generated id at n={}: {}", n, id);
        }
    }

    #[test]
    fn campaign_tag_ids_resolve_to_known_tags() {
        let known: HashSet<Uuid> = TAGS.iter().map(|t| t.id).collect();
        for c in BASE_CAMPAIGNS {
            for tid in c.tag_ids {
                assert!(
                    known.contains(tid),
                    "campaign {} references unknown tag {}",
                    c.id,
                    tid
                );
            }
        }
    }

    #[test]
    fn participants_resolve_to_known_users_and_campaigns() {
        let users: HashSet<Uuid> = BASE_USERS.iter().map(|u| u.id).collect();
        let campaigns: HashSet<Uuid> = BASE_CAMPAIGNS.iter().map(|c| c.id).collect();
        for p in BASE_PARTICIPANTS {
            assert!(users.contains(&p.user_id), "participant {} -> unknown user {}", p.id, p.user_id);
            assert!(campaigns.contains(&p.campaign_id), "participant {} -> unknown campaign {}", p.id, p.campaign_id);
        }
    }

    #[test]
    fn required_tag_names_exist() {
        let names: HashSet<&'static str> = TAGS.iter().map(|t| t.name).collect();
        for n in REQUIRED_TAG_NAMES {
            assert!(names.contains(n), "required tag name {:?} missing from TAGS", n);
        }
    }
}
