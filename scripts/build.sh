#!/usr/bin/env bash
# build/payload.tar.gz を生成する。
# - bench を release ビルド (host で)
# - mitamae cookbook tree に webapp ソース・bench バイナリ・bench.sh 等を staging
# - mitamae バイナリを fetch して同梱
# - 全体を tarball 化
#
# webapp は AMI 上で cargo build される (= ソース同梱・debug 起動)。docs/authoring/build-pipeline.md § 6 参照。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BUILD_DIR="$ROOT/build"
PAYLOAD_DIR="$BUILD_DIR/payload"
CACHE_DIR="$BUILD_DIR/cache"

MITAMAE_VERSION="${MITAMAE_VERSION:-v1.14.4}"
MITAMAE_URL="https://github.com/itamae-kitchen/mitamae/releases/download/${MITAMAE_VERSION}/mitamae-x86_64-linux"

mkdir -p "$BUILD_DIR" "$CACHE_DIR"
rm -rf "$PAYLOAD_DIR"
mkdir -p "$PAYLOAD_DIR/mitamae"

# bash -n は必ず、shellcheck は install されていれば走らせる (advisory)。
# AMI 上で動く bootstrap.sh / bench.sh の syntax 回帰を build 時に検知する。
echo "==> lint shell scripts (bash -n + optional shellcheck)"
SHELL_FILES=(
    "$ROOT"/bench.sh
    "$ROOT"/scripts/*.sh
    "$ROOT"/mitamae/cookbooks/benchwarmer/files/opt/benchwarmer/bootstrap.sh
    "$ROOT"/mitamae/cookbooks/isuwari/files/opt/isuwari/bootstrap.sh
)
for f in "${SHELL_FILES[@]}"; do
    bash -n "$f"
done
if command -v shellcheck >/dev/null 2>&1; then
    shellcheck "${SHELL_FILES[@]}"
else
    echo "    (shellcheck not installed, syntax-only via bash -n)"
fi

# frontend SPA は dev/CI host で pnpm build して dist だけを AMI に載せる (= AMI / SUT に
# Node ランタイムを置かない方針)。lint と並ぶ早い段階で TS 型エラーを潰しておく。
echo "==> preflight pnpm (build host requirement)"
command -v pnpm >/dev/null 2>&1 || {
    echo "ERROR: pnpm is required to build frontend SPA (see frontend/package.json)" >&2
    exit 1
}

echo "==> build frontend SPA (pnpm install --frozen-lockfile + pnpm run build)"
( cd "$ROOT/frontend" && pnpm install --frozen-lockfile && pnpm run build )

echo "==> fetch bench fixtures (ISUCON9 qualify v2 椅子画像、冪等)"
"$ROOT/scripts/fetch-bench-fixtures.sh"

echo "==> generate webapp/sql/seed.sql (${SEED_COUNT:-1500} campaigns)"
# seed-gen は作問インフラ workspace のメンバ。配布物には含めない。
# base/full どちらも seed-data 定数から完全レンダリング。
( cd "$ROOT" && cargo run --release --quiet -p seed-gen -- full \
    --count "${SEED_COUNT:-1500}" \
    --seed "${SEED_PRNG:-0xC0FFEE}" \
    --fixtures "$ROOT/bench/fixtures/images" \
    --out "$ROOT/webapp/sql/seed.sql" )

echo "==> cargo build --release -p bench"
# workspace 化後、bench の成果物は $ROOT/target/release/bench に入る。
( cd "$ROOT" && cargo build --release -p bench )

echo "==> stage mitamae cookbook tree"
rsync -a "$ROOT/mitamae/" "$PAYLOAD_DIR/mitamae/"

echo "==> mitamae binary (${MITAMAE_VERSION})"
if [[ ! -f "$CACHE_DIR/mitamae-${MITAMAE_VERSION}" ]]; then
    curl -fsSL "$MITAMAE_URL" -o "$CACHE_DIR/mitamae-${MITAMAE_VERSION}.download"
    chmod +x "$CACHE_DIR/mitamae-${MITAMAE_VERSION}.download"
    mv "$CACHE_DIR/mitamae-${MITAMAE_VERSION}.download" "$CACHE_DIR/mitamae-${MITAMAE_VERSION}"
fi
cp "$CACHE_DIR/mitamae-${MITAMAE_VERSION}" "$PAYLOAD_DIR/mitamae/mitamae"

echo "==> stage webapp source -> /home/isucon/webapp"
mkdir -p "$PAYLOAD_DIR/mitamae/cookbooks/webapp/files/home/isucon"
rsync -a --exclude='target/' --exclude='*.tmp' \
    "$ROOT/webapp/" \
    "$PAYLOAD_DIR/mitamae/cookbooks/webapp/files/home/isucon/webapp/"

# 配布物に作問者向け unit test (webapp/src/tests.rs + main.rs 末尾の mod 宣言)
# とコメント (実装意図ヒント) を載せない。原本 ($ROOT/webapp) は触らず、
# staging copy だけ編集する。pre-condition は assert せず post-condition のみ
# 守る (= staged src/ から test 用 cfg 属性も `//` 系コメントも消えていること)。
# test-strip は narrow に剥がし、verify は派生形 `#[cfg_attr(test, ...)]` /
# `#[cfg(any(test, ...))]` まで広めに弾く。comment-strip は文字列リテラル
# `"..."` (continuation `\` で多行にまたがるものを含む) の中身は保護しつつ
# `//` 行末まで除去。format drift や新規 test module の追加は build fail で
# 作問者に対応を強制する。
echo "==> strip author-side tests from staged webapp"
STAGED_WEBAPP="$PAYLOAD_DIR/mitamae/cookbooks/webapp/files/home/isucon/webapp"
STAGED_MAIN="$STAGED_WEBAPP/src/main.rs"

rm -f "$STAGED_WEBAPP/src/tests.rs"

perl -i -0777 -pe 's/\n#\[cfg\(test\)\]\s*\nmod tests;\s*\n/\n/' "$STAGED_MAIN"

if [[ -e "$STAGED_WEBAPP/src/tests.rs" ]]; then
    echo "ERROR: $STAGED_WEBAPP/src/tests.rs still present after strip" >&2
    exit 1
fi
# Perl slurp で verify する (行ベース grep だと `#[cfg(any(\n test,\n))]` のような
# 多行 cfg 属性を取り逃がすため)。
while IFS= read -r -d '' f; do
    if ! perl -0777 -ne 'exit 1 if /#\s*\[\s*cfg(?:_attr)?\s*\([^]]*\btest\b[^]]*\)\s*\]/s' "$f"; then
        echo "ERROR: $f contains test-only cfg attribute (incl. multi-line):" >&2
        grep -nE '#[[:space:]]*\[[[:space:]]*cfg(_attr)?' "$f" >&2 || true
        exit 1
    fi
done < <(find "$STAGED_WEBAPP/src" -name '*.rs' -print0)

# raw string (`r"..."` / `r#"..."#` / `br"..."` / `cr"..."`) は comment-stripper が
# 文字列状態を追えないため、出現したら strip 前に fail-loud で止める。本物の
# Rust lexer を持ち込まずに済ませる現実的な防御策。
if grep -nE '(^|[^[:alnum:]_])(b|c)?r#*"' "$STAGED_MAIN" >/dev/null; then
    echo "ERROR: staged main.rs contains raw string literal; comment-strip does not support it:" >&2
    grep -nE '(^|[^[:alnum:]_])(b|c)?r#*"' "$STAGED_MAIN" >&2
    exit 1
fi

echo "==> strip author comments from staged main.rs"
# 文字列横断の state machine。`"..."` 内 (escape `\X` 込み、行継続 `\` で多行に
# またがるものも含む) では `//` を comment 扱いしない。raw string は対応外で
# 上の guard が事前に弾く。`/* */` block comment は現状 main.rs に無いため
# 未対応 (= 出現したら下の verify で fail する; 文字列内 `/*` も同様に弾く)。
# 元から非空白だった行が strip 後に空白だけになった場合はその行ごと drop し、
# 元から空行だった行はそのまま残す。
perl -i -0777 -e '
    use strict;
    my $src = <>;
    my $out = "";
    my $i = 0;
    my $len = length($src);
    my $in_str = 0;
    while ($i < $len) {
        my $c = substr($src, $i, 1);
        if ($in_str) {
            if ($c eq "\\" && $i + 1 < $len) {
                $out .= substr($src, $i, 2);
                $i += 2;
                next;
            }
            $out .= $c;
            $in_str = 0 if $c eq "\"";
            $i++;
        } else {
            if ($c eq "/" && $i + 1 < $len && substr($src, $i + 1, 1) eq "/") {
                while ($i < $len && substr($src, $i, 1) ne "\n") { $i++; }
                next;
            }
            $out .= $c;
            $in_str = 1 if $c eq "\"";
            $i++;
        }
    }
    my @orig_lines = split /\n/, $src, -1;
    my @new_lines  = split /\n/, $out, -1;
    my @final;
    for (my $j = 0; $j < @orig_lines && $j < @new_lines; $j++) {
        my $o = $orig_lines[$j];
        my $n = $new_lines[$j];
        next if $n =~ /^\s*$/ && $o !~ /^\s*$/;
        $n =~ s/[ \t\r]+$//;
        push @final, $n;
    }
    print join("\n", @final);
' "$STAGED_MAIN"

if grep -nE '^[[:space:]]*//' "$STAGED_MAIN" >/dev/null; then
    echo "ERROR: staged main.rs still contains line-leading comments:" >&2
    grep -nE '^[[:space:]]*//' "$STAGED_MAIN" >&2
    exit 1
fi
if grep -nF '/*' "$STAGED_MAIN" >/dev/null; then
    echo "ERROR: staged main.rs contains /* (block comment or string-embedded /*) — unsupported:" >&2
    grep -nF '/*' "$STAGED_MAIN" >&2
    exit 1
fi

# strip 後の staged crate がそのまま compile することを payload 作成時点で確認。
# AMI build まで待つと feedback ループが長くなるので、ここで落とす。target/ は
# payload に混入しないよう CACHE_DIR 配下に逃がす。--locked は同梱の Cargo.lock
# を尊重するため。
echo "==> verify stripped staged webapp compiles (cargo check --locked)"
( cd "$STAGED_WEBAPP" && CARGO_TARGET_DIR="$CACHE_DIR/staged-webapp-target" cargo check --locked --quiet )

# frontend/dist を /home/isucon/webapp/public に staging。webapp の axum が STATIC_DIR
# (= ↑のパス) を参照して ServeDir + index.html fallback で SPA を配信する (nginx を
# 置かない方針: docs/authoring/platform.md 付録 A)。--delete で古い asset chunk が
# 残らないようにする。
echo "==> stage frontend dist -> /home/isucon/webapp/public"
mkdir -p "$PAYLOAD_DIR/mitamae/cookbooks/webapp/files/home/isucon/webapp/public"
rsync -a --delete \
    "$ROOT/frontend/dist/" \
    "$PAYLOAD_DIR/mitamae/cookbooks/webapp/files/home/isucon/webapp/public/"

echo "==> stage bench binary + bench.sh -> /opt/bench"
mkdir -p "$PAYLOAD_DIR/mitamae/cookbooks/bench/files/opt/bench"
cp "$ROOT/target/release/bench" "$PAYLOAD_DIR/mitamae/cookbooks/bench/files/opt/bench/bench"
cp "$ROOT/bench.sh" "$PAYLOAD_DIR/mitamae/cookbooks/bench/files/opt/bench/bench.sh"

echo "==> stage bench fixture zip -> /opt/bench/fixtures/initial.zip"
mkdir -p "$PAYLOAD_DIR/mitamae/cookbooks/bench/files/opt/bench/fixtures"
cp "$ROOT/bench/fixtures/initial.zip" \
    "$PAYLOAD_DIR/mitamae/cookbooks/bench/files/opt/bench/fixtures/initial.zip"

echo "==> stage problem.json -> /opt/benchwarmer/problem.json"
mkdir -p "$PAYLOAD_DIR/mitamae/cookbooks/benchwarmer/files/opt/benchwarmer"
cp "$ROOT/problem.json" \
    "$PAYLOAD_DIR/mitamae/cookbooks/benchwarmer/files/opt/benchwarmer/problem.json"

# benchwarmer / isuwari は ../isunarabe2/ workspace から release build して staging する。
# release artifact が公開された後に GitHub release fetch に切り替える可能性はあるが、
# 現状は隣接 workspace を直接 build する方が手戻りが少ない。
ISUNARABE2_DIR="${ISUNARABE2_DIR:-$ROOT/../isunarabe2}"
test -f "$ISUNARABE2_DIR/Cargo.toml" || {
    echo "ERROR: ISUNARABE2_DIR not found at $ISUNARABE2_DIR (= ../isunarabe2/Cargo.toml が要る)" >&2
    exit 1
}
echo "==> cargo build --release (benchwarmer + isuwari) in $ISUNARABE2_DIR"
( cd "$ISUNARABE2_DIR" && cargo build --release --bin benchwarmer --bin isuwari )

echo "==> stage benchwarmer binary -> /opt/benchwarmer/benchwarmer"
cp "$ISUNARABE2_DIR/target/release/benchwarmer" \
    "$PAYLOAD_DIR/mitamae/cookbooks/benchwarmer/files/opt/benchwarmer/benchwarmer"

echo "==> stage isuwari binary -> /opt/isuwari/isuwari"
mkdir -p "$PAYLOAD_DIR/mitamae/cookbooks/isuwari/files/opt/isuwari"
cp "$ISUNARABE2_DIR/target/release/isuwari" \
    "$PAYLOAD_DIR/mitamae/cookbooks/isuwari/files/opt/isuwari/isuwari"

echo "==> tar czf build/payload.tar.gz"
tar czf "$BUILD_DIR/payload.tar.gz" -C "$BUILD_DIR" payload

echo "==> done"
ls -lh "$BUILD_DIR/payload.tar.gz"
