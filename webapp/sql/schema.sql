-- nrb2026 webapp schema (naive 配布版)
--
-- 設計メモ:
--   * UUID は CHAR(36) で人間可読 (BINARY(16) 化は競技者の最適化余地)
--   * 日時は DATETIME(6) でミリ秒精度。JSON は %Y-%m-%dT%H:%M:%S%.3fZ で出す
--   * collation は utf8mb4_0900_ai_ci (MySQL 8.0 の素直なデフォルト。
--     utf8mb4_bin は sqlx 0.8 の VARCHAR デコードと相性が悪く Vec<u8> として返ってきてしまう)
--   * current_count / status / last_joined_at カラムは持たない (派生計算)
--   * 性能目的の二次 index は付けない (tags.name UNIQUE は name lookup のための例外)
--   * FK 制約は付けない (ISUCON 慣例)
--   * charges.campaign_participant_id に UNIQUE を付けない (= 二重課金 critical の題材)
--
-- 詳細は docs/idea.md を参照。

DROP TABLE IF EXISTS `app_config`;
DROP TABLE IF EXISTS `saved_search_tags`;
DROP TABLE IF EXISTS `saved_searches`;
DROP TABLE IF EXISTS `campaign_tags`;
DROP TABLE IF EXISTS `tags`;
DROP TABLE IF EXISTS `charges`;
DROP TABLE IF EXISTS `campaign_participants`;
DROP TABLE IF EXISTS `campaigns`;
DROP TABLE IF EXISTS `users`;

CREATE TABLE `users` (
    `id` CHAR(36) NOT NULL,
    `name` VARCHAR(100) NOT NULL,
    `credit_limit` INT NOT NULL DEFAULT 60000,
    `created_at` DATETIME(6) NOT NULL,
    PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE `campaigns` (
    `id` CHAR(36) NOT NULL,
    `name` VARCHAR(100) NOT NULL,
    `description` VARCHAR(1000) NOT NULL,
    `price` INT NOT NULL,
    `goal_count` INT NOT NULL,
    `image` LONGBLOB NOT NULL,
    `created_at` DATETIME(6) NOT NULL,
    PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE `campaign_participants` (
    `id` CHAR(36) NOT NULL,
    `campaign_id` CHAR(36) NOT NULL,
    `user_id` CHAR(36) NOT NULL,
    `created_at` DATETIME(6) NOT NULL,
    PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE `charges` (
    `id` CHAR(36) NOT NULL,
    `campaign_participant_id` CHAR(36) NOT NULL,
    `created_at` DATETIME(6) NOT NULL,
    PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE `tags` (
    `id` CHAR(36) NOT NULL,
    `name` VARCHAR(100) NOT NULL,
    `created_at` DATETIME(6) NOT NULL,
    PRIMARY KEY (`id`),
    UNIQUE KEY `uniq_tags_name` (`name`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE `campaign_tags` (
    `campaign_id` CHAR(36) NOT NULL,
    `tag_id` CHAR(36) NOT NULL,
    `created_at` DATETIME(6) NOT NULL,
    PRIMARY KEY (`campaign_id`, `tag_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE `saved_searches` (
    `id` CHAR(36) NOT NULL,
    `user_id` CHAR(36) NOT NULL,
    `created_at` DATETIME(6) NOT NULL,
    PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE `saved_search_tags` (
    `saved_search_id` CHAR(36) NOT NULL,
    `tag_id` CHAR(36) NOT NULL,
    `created_at` DATETIME(6) NOT NULL,
    PRIMARY KEY (`saved_search_id`, `tag_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;

CREATE TABLE `app_config` (
    `name` VARCHAR(64) NOT NULL,
    `value` TEXT NOT NULL,
    PRIMARY KEY (`name`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
