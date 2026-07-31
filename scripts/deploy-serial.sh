#!/bin/sh
set -eu

stage=${1:?usage: deploy-serial.sh <staging|production>}
case "$stage" in staging|production) ;; *) echo "invalid stage" >&2; exit 2;; esac
if [ -f ".env.$stage" ]; then set -a; . "./.env.$stage"; set +a; fi

: "${AWS_PROFILE:?set AWS_PROFILE=ghostpost-deployer}"
caller_arn=$(aws sts get-caller-identity --query Arn --output text)
case "$caller_arn" in *:assumed-role/GhostpostSstDeployer/*) ;; *) echo "refusing AWS changes outside GhostpostSstDeployer" >&2; exit 1;; esac
lock="${TMPDIR:-/tmp}/ghostpost-${stage}-deploy.lock"
if ! mkdir "$lock" 2>/dev/null; then echo "another $stage deployment is active" >&2; exit 1; fi
trap 'rmdir "$lock"' EXIT INT TERM

: "${SCAN_LLM_APPROVAL_MANIFEST_PATH:?required}"
: "${SCAN_PROMPT_SHA256:?required}"
: "${SCAN_SCHEMA_SHA256:?required}"
node scripts/validate-deepseek-manifest.mjs "$SCAN_LLM_APPROVAL_MANIFEST_PATH"
npx sst secret set ScanLlmApprovalManifestJson "$(cat "$SCAN_LLM_APPROVAL_MANIFEST_PATH")" --stage "$stage"

npx sst deploy --stage "$stage" --target Migrations
SST_STAGE="$stage" npx sst shell --stage "$stage" node scripts/run-migration-task.mjs
npx sst deploy --stage "$stage" --target Api
scripts/smoke-health.sh "https://${GHOSTPOST_API_DOMAIN:?required}"
npx sst deploy --stage "$stage" --target Web
