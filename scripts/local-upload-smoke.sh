#!/usr/bin/env bash
# Upload E2E against the Compose stack:
# reserve → presigned PUT straight to MinIO → replayed PUT rejected (412) →
# complete pins the ETag → worker normalizes and purges the raw archive.
# Writes backend/target/e2e-artifacts/local-upload-smoke.json.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

API_BASE="${API_BASE:-http://127.0.0.1:8080}"
FIXTURE="${FIXTURE:-$ROOT/backend/tests/fixtures/x/user_redacted_classic_v1.zip}"
PLATFORM="${PLATFORM:-x}"
CONTENT_TYPE="${CONTENT_TYPE:-application/zip}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$ROOT/backend/target/e2e-artifacts}"

# Load root .env if present (Compose secrets + optional GP_TEST_SESSION).
if [[ -f "$ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$ROOT/.env"
  set +a
fi

die() {
  echo "local-upload-smoke: $*" >&2
  exit 1
}

for cmd in curl python3 docker uuidgen; do
  command -v "$cmd" >/dev/null 2>&1 || die "required command not found: $cmd"
done
[[ -f "$FIXTURE" ]] || die "fixture missing: $FIXTURE"

echo "==> health/ready"
curl -fsS "$API_BASE/health/ready" >/dev/null || die "API not ready at $API_BASE/health/ready"

# Auth: operator-exported native bearer after a real WorkOS exchange. Never bypass auth.
if [[ -z "${GP_TEST_SESSION:-}" ]]; then
  cat >&2 <<'EOF'
local-upload-smoke: GP_TEST_SESSION is not set.

Obtain a native bearer:
  1. GET /v1/auth/authorize?client=native
  2. Complete WorkOS login in the returned authorizationUrl
  3. POST /v1/auth/exchange with client=native, code, state, exchangeSecret
  4. export GP_TEST_SESSION=<session.token from JSON>

Also ensure root .env has WORKOS_* and APP_SESSION_KEYS for the Compose backend.
EOF
  exit 1
fi

uuid() { uuidgen | tr '[:upper:]' '[:lower:]'; }
json_field() { python3 -c "import json,sys; v=json.load(open(sys.argv[1])); [v:=v[k] for k in sys.argv[2:]]; print(v)" "$@"; }
psql_one() { docker compose exec -T postgres psql -U ghostpost -d ghostpost -v ON_ERROR_STOP=1 -At -F $'\t' -c "$1"; }

CONTENT_LENGTH="$(wc -c <"$FIXTURE" | tr -d ' ')"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/gp-upload.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "==> reserve archive import (platform=$PLATFORM length=$CONTENT_LENGTH)"
RESERVE_CODE="$(
  curl -sS -o "$WORK/reserve.json" -w '%{http_code}' \
    -X POST "$API_BASE/v1/archive-imports" \
    -H "Authorization: Bearer ${GP_TEST_SESSION}" \
    -H "Content-Type: application/json" \
    -H "Idempotency-Key: $(uuid)" \
    -d "{\"platform\":\"$PLATFORM\",\"contentLength\":$CONTENT_LENGTH,\"contentType\":\"$CONTENT_TYPE\"}"
)"
[[ "$RESERVE_CODE" == "201" ]] || die "reserve expected 201 got $RESERVE_CODE: $(cat "$WORK/reserve.json")"

python3 - "$WORK/reserve.json" "$WORK/headers.txt" <<'PY'
import json, sys
body = json.load(open(sys.argv[1]))
upload = body["upload"]
assert upload["method"] == "PUT", body
assert upload["headers"] == {"Content-Type": upload["headers"]["Content-Type"], "If-None-Match": "*"}, body
for forbidden in ("rawStorageKey", "rawStorageEtag", "archiveFingerprint", "contentSha256", "etag"):
    assert forbidden not in body, f"leaked {forbidden}: {body}"
# One curl -H argument per signed header; values never hit argv logs.
open(sys.argv[2], "w").write("".join(f"{k}: {v}\n" for k, v in upload["headers"].items()))
PY
IMPORT_ID="$(json_field "$WORK/reserve.json" id)"
UPLOAD_URL="$(json_field "$WORK/reserve.json" upload url)"

# Host is part of the signature: use the URL exactly as minted
# (ARCHIVE_S3_PUBLIC_ENDPOINT=http://localhost:9000 resolves from the host).
put_archive() {
  curl -sS -o "$WORK/put-$1.txt" -w '%{http_code}' -X PUT "$UPLOAD_URL" \
    -H @"$WORK/headers.txt" --upload-file "$2"
}

echo "==> wrong-size body is rejected by the signature"
head -c $((CONTENT_LENGTH - 1)) "$FIXTURE" >"$WORK/short.zip"
SHORT_CODE="$(put_archive short "$WORK/short.zip")"
[[ "$SHORT_CODE" == "403" ]] || die "short PUT expected 403 got $SHORT_CODE: $(head -c 400 "$WORK/put-short.txt")"

echo "==> direct presigned PUT to object store (no API proxy)"
PUT_CODE="$(put_archive exact "$FIXTURE")"
[[ "$PUT_CODE" == "200" ]] || die "PUT expected 200 got $PUT_CODE: $(head -c 400 "$WORK/put-exact.txt")"

echo "==> replayed PUT cannot overwrite"
REPLAY_CODE="$(put_archive replay "$FIXTURE")"
[[ "$REPLAY_CODE" == "412" ]] || die "replayed PUT expected 412 got $REPLAY_CODE: $(head -c 400 "$WORK/put-replay.txt")"

echo "==> complete import"
COMPLETE_CODE="$(
  curl -sS -o "$WORK/complete.json" -w '%{http_code}' \
    -X POST "$API_BASE/v1/archive-imports/${IMPORT_ID}/complete" \
    -H "Authorization: Bearer ${GP_TEST_SESSION}" \
    -H "Idempotency-Key: $(uuid)" \
    -H "Content-Length: 0"
)"
[[ "$COMPLETE_CODE" == "202" ]] || die "complete expected 202 got $COMPLETE_CODE: $(cat "$WORK/complete.json")"

# The worker may already have purged the bytes (and cleared the column), so read
# the pin from the normalization job it was handed. Never exposed by the API.
echo "==> worker was handed the completion-time ETag pin"
PINNED_ETAG="$(psql_one "SELECT payload->>'etag' FROM work_items WHERE kind='import.normalize' AND payload->>'import_id'='${IMPORT_ID}'")"
[[ -n "$PINNED_ETAG" ]] || die "import.normalize job has no ETag pin for $IMPORT_ID"

echo "==> wait for normalization and raw purge"
STATUS=""
for _ in $(seq 1 60); do
  ROW="$(psql_one "SELECT status, raw_deleted_at IS NOT NULL, coalesce(item_count,0) FROM archive_imports WHERE id='${IMPORT_ID}'::uuid")"
  STATUS="$(cut -f1 <<<"$ROW")"
  [[ "$STATUS" == "ready" || "$STATUS" == "rejected" || "$STATUS" == "failed" ]] && break
  sleep 1
done
[[ "$STATUS" == "ready" ]] || die "import ended in '$STATUS' (row: $ROW)"
RAW_DELETED="$(cut -f2 <<<"$ROW")"
ITEM_COUNT="$(cut -f3 <<<"$ROW")"
[[ "$RAW_DELETED" == "t" ]] || die "raw archive not purged after normalization"
STORAGE_KEY="$(psql_one "SELECT raw_storage_key FROM archive_imports WHERE id='${IMPORT_ID}'::uuid")"
if docker compose run --rm --entrypoint /bin/sh minio-init -c \
  "mc alias set local http://minio:9000 \"\${MINIO_ROOT_USER:-ghostpost}\" \"\${MINIO_ROOT_PASSWORD:-ghostpostsecret}\" >/dev/null && mc stat local/ghostpost-archives/$STORAGE_KEY" >/dev/null 2>&1; then
  die "raw archive object still exists after purge"
fi

echo "==> anonymous GET is denied"
ANON_CODE="$(curl -sS -o /dev/null -w '%{http_code}' "http://localhost:9000/ghostpost-archives/${STORAGE_KEY}" || true)"
case "$ANON_CODE" in 200|206) die "unauthenticated GET unexpectedly succeeded ($ANON_CODE)" ;; esac

echo "==> oversize reservation is rejected"
OVER_CODE="$(
  curl -sS -o "$WORK/over.json" -w '%{http_code}' \
    -X POST "$API_BASE/v1/archive-imports" \
    -H "Authorization: Bearer ${GP_TEST_SESSION}" \
    -H "Content-Type: application/json" \
    -H "Idempotency-Key: $(uuid)" \
    -d '{"platform":"x","contentLength":3147483648,"contentType":"application/zip"}'
)"
[[ "$OVER_CODE" == "422" ]] || die "oversize reserve expected 422 got $OVER_CODE: $(cat "$WORK/over.json")"

mkdir -p "$ARTIFACT_DIR"
cat >"$ARTIFACT_DIR/local-upload-smoke.json" <<EOF
{
  "importId": "$IMPORT_ID",
  "platform": "$PLATFORM",
  "contentLength": $CONTENT_LENGTH,
  "putShortBody": $SHORT_CODE,
  "putExact": $PUT_CODE,
  "putReplay": $REPLAY_CODE,
  "complete": $COMPLETE_CODE,
  "finalStatus": "$STATUS",
  "itemCount": $ITEM_COUNT,
  "rawPurged": true,
  "anonymousGet": $ANON_CODE,
  "oversizeReserve": $OVER_CODE
}
EOF
echo "local-upload-smoke: OK (import=$IMPORT_ID items=$ITEM_COUNT); artifact: $ARTIFACT_DIR/local-upload-smoke.json"
