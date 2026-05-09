#!/usr/bin/env bash
# bench fixture (ISUCON9 qualify v2 の椅子画像 20000 枚) を DL & 展開する。
#
# 出典・License は bench/fixtures/README.md を参照。
# scripts/build.sh から冪等に呼ばれる (sentinel あり、再 DL しない)。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

FIX_DIR="$ROOT/bench/fixtures"
ZIP="$FIX_DIR/initial.zip"
IMG_DIR="$FIX_DIR/images"
SENTINEL="$IMG_DIR/.fetched"
URL="https://github.com/isucon/isucon9-qualify/releases/download/v2/initial.zip"
# 9q v2 initial.zip の固定 sha256 (バイナリ改竄/破損検出用)
EXPECTED_SHA256="74b43ed843edfa2dd2f651ba8cdf81f1ff524ec24b623cf83a0f24ff07952804"
EXPECTED_COUNT=20000

mkdir -p "$FIX_DIR"

if [[ -f "$SENTINEL" ]]; then
    echo "==> bench fixtures already fetched ($IMG_DIR), skip"
    exit 0
fi

if [[ ! -f "$ZIP" ]]; then
    echo "==> downloading $URL"
    curl -fL --retry 3 --retry-delay 2 -o "$ZIP.tmp" "$URL"
    mv "$ZIP.tmp" "$ZIP"
fi

echo "==> verify sha256"
echo "${EXPECTED_SHA256}  ${ZIP}" | sha256sum -c -

if ! command -v unzip >/dev/null 2>&1; then
    echo "ERROR: unzip is not installed (apt-get install unzip)" >&2
    exit 1
fi

echo "==> unzip $ZIP -> $FIX_DIR (temp 経由で atomic 化)"
TMP_DIR="$FIX_DIR/.unpacking"
rm -rf "$IMG_DIR" "$FIX_DIR/v3_initial_data" "$TMP_DIR"
mkdir -p "$TMP_DIR"
unzip -qq "$ZIP" -d "$TMP_DIR"
# 9q v2 zip は v3_initial_data/ に jpg を展開する
mv "$TMP_DIR/v3_initial_data" "$IMG_DIR"
rmdir "$TMP_DIR"

COUNT="$(find "$IMG_DIR" -maxdepth 1 -type f -name '*.jpg' | wc -l)"
if [[ "$COUNT" -ne "$EXPECTED_COUNT" ]]; then
    echo "ERROR: expected ${EXPECTED_COUNT} .jpg in $IMG_DIR, got $COUNT" >&2
    exit 1
fi

# 仕様: 全 fixture は 200 KiB 以下 (= POST /campaigns の image 上限)
if find "$IMG_DIR" -maxdepth 1 -type f -name '*.jpg' -size +204800c -print -quit | grep -q .; then
    echo "ERROR: some fixture jpg exceeds 200 KiB (= API image limit)" >&2
    find "$IMG_DIR" -maxdepth 1 -type f -name '*.jpg' -size +204800c | head -3
    exit 1
fi

touch "$SENTINEL"
echo "==> fetched $COUNT .jpg fixtures"
