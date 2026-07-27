#!/bin/sh
# Waits for MinIO, creates versioned private bucket used by local backend.
# Uses mc against the compose service DNS name "minio".
#
# Lifecycle: best-effort local parity with production ≤24h raw retention
# (Expiration.Days: 1 + NoncurrentVersionExpiration). MinIO lifecycle JSON
# support varies by RELEASE; versioning is mandatory and asserted below.
# Plan 009 enforces full AWS lifecycle authoritatively.
set -eu

BUCKET="${ARCHIVE_BUCKET:-ghostpost-archives}"
ENDPOINT="${MINIO_ENDPOINT:-http://minio:9000}"
ROOT_USER="${MINIO_ROOT_USER:-ghostpost}"
ROOT_PASSWORD="${MINIO_ROOT_PASSWORD:-ghostpostsecret}"

i=0
until mc alias set local "$ENDPOINT" "$ROOT_USER" "$ROOT_PASSWORD" >/dev/null 2>&1 \
  && mc ready local >/dev/null 2>&1; do
  i=$((i + 1))
  if [ "$i" -ge 60 ]; then
    echo "minio-init: MinIO not ready at $ENDPOINT after ${i}s" >&2
    exit 1
  fi
  sleep 1
done

mc mb -p "local/${BUCKET}" >/dev/null 2>&1 || true
mc version enable "local/${BUCKET}"

info="$(mc version info "local/${BUCKET}")"
echo "$info"
# mc prints "versioning is enabled" / status Enabled depending on release.
# Prefer pure shell — minio/mc image may lack grep.
case "$info" in
  *[Ee]nabled*|*"versioning is enabled"*) ;;
  *)
    echo "minio-init: versioning not enabled on local/${BUCKET}" >&2
    exit 1
    ;;
esac

# Best-effort lifecycle (ignore failure on older mc/minio).
lifecycle_tmp="$(mktemp)"
cat >"$lifecycle_tmp" <<'EOF'
{
  "Rules": [
    {
      "ID": "ghostpost-raw-24h",
      "Status": "Enabled",
      "Filter": { "Prefix": "" },
      "Expiration": { "Days": 1 },
      "NoncurrentVersionExpiration": { "NoncurrentDays": 1 }
    }
  ]
}
EOF
if ! mc ilm import "local/${BUCKET}" <"$lifecycle_tmp" >/dev/null 2>&1; then
  echo "minio-init: lifecycle import skipped (best-effort; Plan 009 is authoritative)" >&2
fi
rm -f "$lifecycle_tmp"

# Never set public anonymous download policy.
exit 0
