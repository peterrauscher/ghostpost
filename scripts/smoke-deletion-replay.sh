#!/bin/sh
set -eu
stage=${1:?usage: smoke-deletion-replay.sh <stage>}
: "${RESTORE_POINT:?required}"
: "${RESTORE_GUARD_TOKEN:?required}"
: "${RESTORE_DATABASE_URL:?required}"
: "${DELETED_TENANT_ID:?required}"
: "${DELETED_USER_ID:?required}"
SST_STAGE="$stage" npx sst shell --stage "$stage" node scripts/run-deletion-replay.mjs
remaining=$(psql "$RESTORE_DATABASE_URL" -v ON_ERROR_STOP=1 -At -v tenant="$DELETED_TENANT_ID" -v user="$DELETED_USER_ID" -c "SELECT count(*) FROM users WHERE tenant_id=:'tenant'::uuid AND id=:'user'::uuid")
[ "$remaining" = 0 ] || { echo 'deleted user remains readable' >&2; exit 1; }
printf 'deletion replay smoke passed\n'
