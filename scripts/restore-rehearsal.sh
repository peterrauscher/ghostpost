#!/bin/sh
set -eu

stage=${1:?usage: restore-rehearsal.sh <staging|production>}
case "$stage" in staging|production) ;; *) echo "invalid stage" >&2; exit 2;; esac
if [ -f ".env.$stage" ]; then set -a; . "./.env.$stage"; set +a; fi

: "${AWS_PROFILE:?set AWS_PROFILE=ghostpost-deployer}"
caller_arn=$(aws sts get-caller-identity --query Arn --output text)
case "$caller_arn" in *:assumed-role/GhostpostSstDeployer/*) ;; *) echo "refusing AWS changes outside GhostpostSstDeployer" >&2; exit 1;; esac
: "${RESTORE_POINT:?RFC3339 restore point required}"
: "${RESTORE_DATABASE_URL_MIGRATOR:?isolated restored database migrator URL required}"
: "${RESTORE_DATABASE_URL_APP:?isolated restored database app URL required}"
: "${ALLOW_RESTORE_CUTOVER:?set to YES after reviewing isolated restore target}"
[ "$ALLOW_RESTORE_CUTOVER" = YES ] || { echo 'restore cutover not authorized' >&2; exit 1; }

# Guard is UNLOGGED and absent after physical restore. Seed only restored database.
psql "$RESTORE_DATABASE_URL_MIGRATOR" -v ON_ERROR_STOP=1 -f backend/migrations/20260727000015_restore_guard.sql >/dev/null
guard_token=$(openssl rand -hex 32)
guard_hash=$(printf '%s' "$guard_token" | openssl dgst -sha256 -binary | xxd -p -c 256)
psql "$RESTORE_DATABASE_URL_MIGRATOR" -v ON_ERROR_STOP=1 -v token_hash="$guard_hash" -c "INSERT INTO ghostpost_restore_guard(singleton,token_hash) VALUES (true,decode(:'token_hash','hex')) ON CONFLICT(singleton) DO UPDATE SET token_hash=excluded.token_hash,updated_at=now()" >/dev/null

RESTORE_GUARD_TOKEN="$guard_token" RESTORE_DATABASE_URL="$RESTORE_DATABASE_URL_MIGRATOR" SST_STAGE="$stage" \
  npx sst shell --stage "$stage" node scripts/run-deletion-replay.mjs
unset guard_token guard_hash

# Cut over only after replay exits zero; subsequent serial deploy migrates before API rollout.
npx sst secret set DatabaseUrlMigrator "$RESTORE_DATABASE_URL_MIGRATOR" --stage "$stage"
npx sst secret set DatabaseUrlApp "$RESTORE_DATABASE_URL_APP" --stage "$stage"
scripts/deploy-serial.sh "$stage"
