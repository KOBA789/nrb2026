//! webapp の小さいユニットテスト。
//! `scripts/build.sh` が staging 時に本ファイルと main.rs 末尾の
//! `#[cfg(test)] mod tests;` を自動で剥がす (= 配布物には載らない)。手で消す必要は無い。

use super::*;

#[test]
fn fmt_dt_zero_ms() {
    let dt = NaiveDateTime::parse_from_str("2024-05-01 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    assert_eq!(fmt_dt(dt), "2024-05-01T12:00:00.000Z");
}

#[test]
fn fmt_dt_with_ms() {
    let dt =
        NaiveDateTime::parse_from_str("2024-05-01 12:00:00.123", "%Y-%m-%d %H:%M:%S%.f").unwrap();
    assert_eq!(fmt_dt(dt), "2024-05-01T12:00:00.123Z");
}

#[test]
fn parse_db_url_basic() {
    let db = parse_db_url("mysql://isucon:isucon@127.0.0.1:3307/nrb2026");
    assert_eq!(db.host, "127.0.0.1");
    assert_eq!(db.port, 3307);
    assert_eq!(db.user, "isucon");
    assert_eq!(db.password, "isucon");
    assert_eq!(db.database, "nrb2026");
}

#[test]
fn parse_db_url_default_port() {
    let db = parse_db_url("mysql://isucon:isucon@127.0.0.1/nrb2026");
    assert_eq!(db.port, 3306);
}

// === validate_jpeg_image_b64 ===

fn b64_encode(bytes: &[u8]) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    STANDARD.encode(bytes)
}

#[test]
fn validate_jpeg_image_b64_ok() {
    let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    let got = validate_jpeg_image_b64(&b64_encode(&bytes)).expect("ok");
    assert_eq!(got, bytes);
}

#[test]
fn validate_jpeg_image_b64_invalid_base64() {
    let r = validate_jpeg_image_b64("!!!not-base64!!!");
    assert!(matches!(r, Err(AppError::BadRequest)));
}

#[test]
fn validate_jpeg_image_b64_empty_after_decode() {
    let r = validate_jpeg_image_b64("");
    assert!(matches!(r, Err(AppError::BadRequest)));
}

#[test]
fn validate_jpeg_image_b64_at_boundary_204800() {
    let mut bytes = vec![0u8; 204_800];
    bytes[0] = 0xFF;
    bytes[1] = 0xD8;
    bytes[2] = 0xFF;
    assert!(validate_jpeg_image_b64(&b64_encode(&bytes)).is_ok());
}

#[test]
fn validate_jpeg_image_b64_too_large_204801() {
    let mut bytes = vec![0u8; 204_801];
    bytes[0] = 0xFF;
    bytes[1] = 0xD8;
    bytes[2] = 0xFF;
    let r = validate_jpeg_image_b64(&b64_encode(&bytes));
    assert!(matches!(r, Err(AppError::PayloadTooLarge)));
}

#[test]
fn validate_jpeg_image_b64_magic_mismatch_png() {
    let bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let r = validate_jpeg_image_b64(&b64_encode(&bytes));
    assert!(matches!(r, Err(AppError::BadRequest)));
}

#[test]
fn validate_jpeg_image_b64_size_precedes_magic() {
    // non-JPEG でも 204_801 byte なら 413 (size precedence)
    let bytes = vec![0u8; 204_801];
    let r = validate_jpeg_image_b64(&b64_encode(&bytes));
    assert!(matches!(r, Err(AppError::PayloadTooLarge)));
}

#[test]
fn validate_jpeg_image_b64_short_input_no_panic() {
    // 1 byte / 2 byte は magic check で短尺ガード (panic しない、BadRequest)
    assert!(matches!(
        validate_jpeg_image_b64(&b64_encode(&[0xFF])),
        Err(AppError::BadRequest)
    ));
    assert!(matches!(
        validate_jpeg_image_b64(&b64_encode(&[0xFF, 0xD8])),
        Err(AppError::BadRequest)
    ));
}

#[test]
fn validate_jpeg_image_b64_canonical_padding_accepted() {
    // "/9j/4A==" は base64 STANDARD canonical padding (4 byte → 0xFF D8 FF E0)
    assert!(validate_jpeg_image_b64("/9j/4A==").is_ok());
}

#[test]
fn validate_jpeg_image_b64_missing_padding_rejected() {
    // STANDARD は RequireCanonical なので padding 欠落は BadRequest
    let r = validate_jpeg_image_b64("/9j/4A");
    assert!(matches!(r, Err(AppError::BadRequest)));
}

// === AppError → HTTP status mapping ===

#[test]
fn app_error_payment_required_maps_to_402() {
    let resp = AppError::PaymentRequired.into_response();
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// === validate_price (POST /campaigns 仕様: 2000 ≤ price ≤ 20000) ===

#[test]
fn validate_price_below_min_rejected() {
    assert!(matches!(validate_price(1999), Err(AppError::BadRequest)));
    assert!(matches!(validate_price(0), Err(AppError::BadRequest)));
    assert!(matches!(validate_price(-1), Err(AppError::BadRequest)));
}

#[test]
fn validate_price_min_boundary_accepted() {
    assert!(validate_price(2000).is_ok());
}

#[test]
fn validate_price_max_boundary_accepted() {
    assert!(validate_price(20000).is_ok());
}

#[test]
fn validate_price_above_max_rejected() {
    assert!(matches!(validate_price(20001), Err(AppError::BadRequest)));
    assert!(matches!(validate_price(1_000_000), Err(AppError::BadRequest)));
    assert!(matches!(validate_price(i32::MAX), Err(AppError::BadRequest)));
}

#[test]
fn validate_price_typical_ok() {
    assert!(validate_price(10000).is_ok());
}
