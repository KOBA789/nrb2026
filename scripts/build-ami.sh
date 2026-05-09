#!/usr/bin/env bash
# scripts/build.sh で生成した payload を Packer で AMI に焼く。
# AWS credentials は環境変数または ~/.aws/credentials を期待。
# docs/authoring/build-pipeline.md § 6 参照。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

"$ROOT/scripts/build.sh"

if ! command -v packer >/dev/null 2>&1; then
    echo "ERROR: packer is not installed. https://developer.hashicorp.com/packer/install" >&2
    exit 1
fi

if ! aws sts get-caller-identity >/dev/null 2>&1; then
    echo "ERROR: AWS credentials not configured. AWS_PROFILE / env vars / aws configure を確認してください。" >&2
    exit 1
fi

cd "$ROOT/packer"
packer init ami.pkr.hcl
packer build "$@" ami.pkr.hcl
