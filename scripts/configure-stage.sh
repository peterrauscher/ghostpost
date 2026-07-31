#!/bin/sh
set -eu
stage=${1:?usage: configure-stage.sh <staging|production>}
case "$stage" in staging|production) ;; *) echo "invalid stage" >&2; exit 2;; esac
: "${AWS_PROFILE:=ghostpost-deployer}"
export AWS_PROFILE
caller_arn=$(aws sts get-caller-identity --query Arn --output text)
case "$caller_arn" in *:assumed-role/GhostpostSstDeployer/*) ;; *) echo "refusing AWS changes outside GhostpostSstDeployer" >&2; exit 1;; esac

prompt() { printf '%s: ' "$1" >&2; IFS= read -r REPLY; [ -n "$REPLY" ] || { echo "$1 is required" >&2; exit 1; }; }
secret() {
  name=$1; label=$2
  printf '%s (hidden): ' "$label" >&2
  trap 'stty echo; printf "\n" >&2' EXIT INT TERM
  stty -echo
  IFS= read -r value
  stty echo
  trap - EXIT INT TERM
  printf '\n' >&2
  [ -n "$value" ] || { echo "$label is required" >&2; exit 1; }
  npx sst secret set "$name" "$value" --stage "$stage" >/dev/null
  unset value
}

prompt 'AWS region'; region=$REPLY
prompt 'Web domain (hostname only)'; app_domain=$REPLY
prompt 'API domain (hostname only)'; api_domain=$REPLY
prompt 'Registrable cookie site'; cookie_site=$REPLY
prompt 'Postgres PITR days'; pitr_days=$REPLY
prompt 'Deletion-ledger retention days'; ledger_days=$REPLY
prompt 'Committed source SHA'; git_sha=$REPLY
prompt 'Approved DeepSeek manifest path'; approval_path=$REPLY
prompt 'Scan prompt SHA-256'; prompt_hash=$REPLY
prompt 'Scan schema SHA-256'; schema_hash=$REPLY

case "$app_domain$api_domain$cookie_site$region$git_sha$prompt_hash$schema_hash" in *[!A-Za-z0-9._:-]*) echo 'invalid non-secret input' >&2; exit 1;; esac
[ -f "$approval_path" ] || { echo 'approval manifest not found' >&2; exit 1; }
case "$approval_path" in *[!A-Za-z0-9_./-]*) echo 'approval path must not contain spaces or shell metacharacters' >&2; exit 1;; esac
export AWS_REGION=$region GHOSTPOST_APP_DOMAIN=$app_domain GHOSTPOST_API_DOMAIN=$api_domain GHOSTPOST_COOKIE_SITE=$cookie_site
export DATABASE_APP_ROLE=ghostpost_app POSTGRES_PITR_RETENTION_DAYS=$pitr_days DELETION_LEDGER_RETENTION_DAYS=$ledger_days GIT_SHA=$git_sha
export SCAN_LLM_APPROVAL_MANIFEST_PATH=$approval_path SCAN_PROMPT_SHA256=$prompt_hash SCAN_SCHEMA_SHA256=$schema_hash
node scripts/validate-deepseek-manifest.mjs "$approval_path"

secret DatabaseUrlMigrator 'Neon direct migrator URL'
secret DatabaseUrlApp 'Neon direct app URL'
secret WorkosApiKey 'WorkOS API key'
secret WorkosClientId 'WorkOS client ID'
secret WorkosWebhookSecret 'WorkOS webhook secret'
secret WorkosCookiePassword 'WorkOS cookie password'
secret AppSessionKeys 'App session key JSON'
secret DeepseekApiKey 'DeepSeek API key'
secret ArchiveFingerprintKeys 'Archive fingerprint key JSON'
secret ScanBatchHmacKeys 'Scan batch HMAC key JSON'
npx sst secret set ScanLlmApprovalManifestJson "$(cat "$approval_path")" --stage "$stage" >/dev/null

umask 077
cat > ".env.$stage" <<EOF
AWS_PROFILE=$AWS_PROFILE
AWS_REGION=$region
GHOSTPOST_APP_DOMAIN=$app_domain
GHOSTPOST_API_DOMAIN=$api_domain
GHOSTPOST_COOKIE_SITE=$cookie_site
DATABASE_APP_ROLE=ghostpost_app
POSTGRES_PITR_RETENTION_DAYS=$pitr_days
DELETION_LEDGER_RETENTION_DAYS=$ledger_days
GIT_SHA=$git_sha
RESTORE_REPLAY_PENDING=false
SCAN_LLM_APPROVAL_MANIFEST_PATH=$approval_path
SCAN_PROMPT_SHA256=$prompt_hash
SCAN_SCHEMA_SHA256=$schema_hash
EOF
printf 'Configured %s; non-secret settings saved to ignored .env.%s\n' "$stage" "$stage"
