#!/usr/bin/env bash
# Focused durable-upload E2E against the Compose stack (Plans 003–004).
# Reserve → signed multipart POST → complete → prove pinned MinIO version.
# Does not require scan/DeepSeek or import status ready.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

API_BASE="${API_BASE:-http://127.0.0.1:8080}"
FIXTURE="${FIXTURE:-$ROOT/backend/tests/fixtures/x/user_redacted_classic_v1.zip}"
PLATFORM="${PLATFORM:-x}"
CONTENT_TYPE="${CONTENT_TYPE:-application/zip}"

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

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

need_cmd curl
need_cmd python3
need_cmd docker
need_cmd uuidgen

[[ -f "$FIXTURE" ]] || die "fixture missing: $FIXTURE"

echo "==> health/ready"
curl -fsS "$API_BASE/health/ready" >/dev/null \
  || die "API not ready at $API_BASE/health/ready"

# Auth: Plan 003 local harness — operator-exported native bearer after exchange.
# Fail closed; never bypass auth.
if [[ -z "${GP_TEST_SESSION:-}" ]]; then
  cat >&2 <<'EOF'
local-upload-smoke: GP_TEST_SESSION is not set.

Obtain a native bearer via Plan 003:
  1. GET /v1/auth/authorize?client=native
  2. Complete WorkOS login in the returned authorizationUrl
  3. POST /v1/auth/exchange with client=native, code, state, exchangeSecret
  4. export GP_TEST_SESSION=<session.token from JSON>

Preferred automated path (ignored test documents env requirements):
  cargo test --locked --manifest-path backend/Cargo.toml auth::local_session_fixture -- --ignored --nocapture

Also ensure root .env has WORKOS_* and APP_SESSION_KEYS for the Compose backend.
EOF
  exit 1
fi

CONTENT_LENGTH="$(wc -c <"$FIXTURE" | tr -d ' ')"
IDEM_RESERVE="$(uuidgen | tr '[:upper:]' '[:lower:]')"
IDEM_COMPLETE="$(uuidgen | tr '[:upper:]' '[:lower:]')"
TMPDIR="${TMPDIR:-/tmp}"
RESERVE_JSON="$(mktemp "$TMPDIR/gp-reserve.XXXXXX.json")"
COMPLETE_JSON="$(mktemp "$TMPDIR/gp-complete.XXXXXX.json")"
FIELDS_DIR="$(mktemp -d "$TMPDIR/gp-fields.XXXXXX")"
trap 'rm -f "$RESERVE_JSON" "$COMPLETE_JSON"; rm -rf "$FIELDS_DIR"' EXIT

echo "==> reserve archive import (platform=$PLATFORM length=$CONTENT_LENGTH)"
HTTP_CODE="$(
  curl -sS -o "$RESERVE_JSON" -w '%{http_code}' \
    -X POST "$API_BASE/v1/archive-imports" \
    -H "Authorization: Bearer ${GP_TEST_SESSION}" \
    -H "Content-Type: application/json" \
    -H "Idempotency-Key: ${IDEM_RESERVE}" \
    -d "$(python3 -c "import json; print(json.dumps({'platform':'''$PLATFORM''','contentLength':int('''$CONTENT_LENGTH'''),'contentType':'''$CONTENT_TYPE'''}))")"
)"
[[ "$HTTP_CODE" == "201" ]] || die "reserve expected 201 got $HTTP_CODE: $(cat "$RESERVE_JSON")"

python3 - "$RESERVE_JSON" <<'PY'
import json, sys
body = json.load(open(sys.argv[1]))
assert "id" in body, body
upload = body.get("upload") or {}
assert upload.get("method") == "POST", body
assert upload.get("url"), body
assert isinstance(upload.get("fields"), dict) and upload["fields"], body
for forbidden in (
    "rawStorageKey",
    "rawStorageVersionId",
    "archiveFingerprint",
    "contentSha256",
    "etag",
):
    assert forbidden not in body, f"leaked {forbidden}: {body}"
print(body["id"])
print(upload["url"])
PY

IMPORT_ID="$(python3 -c "import json; print(json.load(open('$RESERVE_JSON'))['id'])")"
UPLOAD_URL="$(python3 -c "import json; print(json.load(open('$RESERVE_JSON'))['upload']['url'])")"

# Write each signed field to a file so curl -F can assemble multipart without shell-escaping secrets in argv logs.
python3 - "$RESERVE_JSON" "$FIELDS_DIR" <<'PY'
import json, os, sys
fields = json.load(open(sys.argv[1]))["upload"]["fields"]
out = sys.argv[2]
names = []
for k, v in fields.items():
    path = os.path.join(out, f"{len(names):04d}.field")
    with open(path, "w", encoding="utf-8") as f:
        f.write(str(v))
    names.append(k)
with open(os.path.join(out, "order.txt"), "w", encoding="utf-8") as f:
    f.write("\n".join(names))
PY

CURL_FIELDS=()
while IFS= read -r name; do
  [[ -z "$name" ]] && continue
  idx="$(printf '%04d' "${#CURL_FIELDS[@]}")"
  # Prefer order file index from python naming
  :
done <"$FIELDS_DIR/order.txt"

i=0
while IFS= read -r name; do
  [[ -z "$name" ]] && continue
  path="$(printf '%s/%04d.field' "$FIELDS_DIR" "$i")"
  CURL_FIELDS+=(-F "${name}=<${path}")
  i=$((i + 1))
done <"$FIELDS_DIR/order.txt"
CURL_FIELDS+=(-F "file=@${FIXTURE};type=${CONTENT_TYPE}")

echo "==> direct multipart POST to object store (no API proxy)"
# Host rewrite: signed URL may point at http://minio:9000/... from inside Compose.
# From the host, reach MinIO on loopback:9000.
HOST_UPLOAD_URL="$(
  python3 -c "from urllib.parse import urlparse, urlunparse; u=urlparse('''$UPLOAD_URL''');
host=u.hostname or '';
netloc=('127.0.0.1:9000' if host in ('minio','localhost') else u.netloc);
print(urlunparse((u.scheme, netloc, u.path, u.params, u.query, u.fragment)))"
)"

UPLOAD_CODE="$(
  curl -sS -o /tmp/gp-upload-body.txt -w '%{http_code}' \
    -X POST "$HOST_UPLOAD_URL" \
    "${CURL_FIELDS[@]}"
)" || true
# S3/MinIO success is typically 204/201/200
case "$UPLOAD_CODE" in
  200|201|204) ;;
  *)
    die "object store POST expected 2xx got $UPLOAD_CODE: $(head -c 500 /tmp/gp-upload-body.txt 2>/dev/null || true)"
    ;;
esac

echo "==> complete import"
COMPLETE_CODE="$(
  curl -sS -o "$COMPLETE_JSON" -w '%{http_code}' \
    -X POST "$API_BASE/v1/archive-imports/${IMPORT_ID}/complete" \
    -H "Authorization: Bearer ${GP_TEST_SESSION}" \
    -H "Idempotency-Key: ${IDEM_COMPLETE}" \
    -H "Content-Length: 0"
)"
[[ "$COMPLETE_CODE" == "202" ]] || die "complete expected 202 got $COMPLETE_CODE: $(cat "$COMPLETE_JSON")"

python3 - "$COMPLETE_JSON" "$IMPORT_ID" <<'PY'
import json, sys
body = json.load(open(sys.argv[1]))
expected = sys.argv[2]
assert body.get("id") == expected, body
for forbidden in (
    "rawStorageKey",
    "rawStorageVersionId",
    "archiveFingerprint",
    "contentSha256",
    "etag",
    "uploadExpiresAt",
    "rawDeleteAfter",
):
    assert forbidden not in body, f"leaked {forbidden}: {body}"
print("complete status:", body.get("status"))
PY

echo "==> internal pin proof via Postgres + mc (not public API)"
# Query internal columns only inside this local smoke — never log key/version beyond mc.
ROW="$(
  docker compose exec -T postgres psql -U ghostpost -d ghostpost -v ON_ERROR_STOP=1 -At -F $'\t' -c \
    "SELECT raw_storage_key, raw_storage_version_id FROM archive_imports WHERE id = '${IMPORT_ID}'::uuid;"
)"
STORAGE_KEY="$(printf '%s' "$ROW" | cut -f1)"
VERSION_ID="$(printf '%s' "$ROW" | cut -f2)"
[[ -n "$STORAGE_KEY" && "$STORAGE_KEY" != "" ]] || die "raw_storage_key empty for $IMPORT_ID"
[[ -n "$VERSION_ID" && "$VERSION_ID" != "" ]] || die "raw_storage_version_id empty for $IMPORT_ID"

docker compose run --rm --entrypoint /bin/sh minio-init -c \
  "mc alias set local http://minio:9000 \"\${MINIO_ROOT_USER:-ghostpost}\" \"\${MINIO_ROOT_PASSWORD:-ghostpostsecret}\" >/dev/null && \
   mc stat --version-id $(printf '%q' "$VERSION_ID") local/ghostpost-archives/$(printf '%q' "$STORAGE_KEY")" \
  >/dev/null

echo "==> anonymous access must not be granted"
ANON="$(
  docker compose run --rm --entrypoint /bin/sh minio-init -c \
    'mc alias set local http://minio:9000 "${MINIO_ROOT_USER:-ghostpost}" "${MINIO_ROOT_PASSWORD:-ghostpostsecret}" >/dev/null; \
     mc anonymous get local/ghostpost-archives 2>/dev/null || true'
)"
if echo "$ANON" | grep -Eiq 'download|public|readwrite|readonly'; then
  # "Access permission for ... is \`none\`" is OK
  if ! echo "$ANON" | grep -Eiq 'none|private'; then
    die "bucket appears anonymously readable: $ANON"
  fi
fi

# Unauthenticated GET without signature should fail.
UNAUTH_CODE="$(
  curl -sS -o /dev/null -w '%{http_code}' \
    "http://127.0.0.1:9000/ghostpost-archives/${STORAGE_KEY}" || true
)"
case "$UNAUTH_CODE" in
  200|206) die "unauthenticated GET unexpectedly succeeded ($UNAUTH_CODE)" ;;
esac

echo "==> oversize reservation must be rejected"
OVER_CODE="$(
  curl -sS -o /tmp/gp-over.json -w '%{http_code}' \
    -X POST "$API_BASE/v1/archive-imports" \
    -H "Authorization: Bearer ${GP_TEST_SESSION}" \
    -H "Content-Type: application/json" \
    -H "Idempotency-Key: $(uuidgen | tr '[:upper:]' '[:lower:]')" \
    -d '{"platform":"x","contentLength":3147483648,"contentType":"application/zip"}'
)"
[[ "$OVER_CODE" == "422" || "$OVER_CODE" == "400" ]] \
  || die "oversize reserve expected 4xx got $OVER_CODE: $(cat /tmp/gp-over.json)"

echo "local-upload-smoke: OK (import=$IMPORT_ID pinned version present; response sanitized)"
