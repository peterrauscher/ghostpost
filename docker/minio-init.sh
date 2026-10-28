#!/bin/sh
# Waits for MinIO and creates the private archive bucket used by the local backend.
# Uses mc against the compose service DNS name "minio".
#
# The bucket is intentionally unversioned, matching production R2: uploads are
# create-once presigned PUTs pinned by ETag. Lifecycle mirrors production's
# 24h raw-archive backstop and is best-effort because MinIO support varies.
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

# Best-effort lifecycle (ignore failure on older mc/minio).
lifecycle_tmp="$(mktemp)"
cat >"$lifecycle_tmp" <<'EOF'
{
  "Rules": [
    {
      "ID": "ghostpost-raw-archives-24h",
      "Status": "Enabled",
      "Filter": { "Prefix": "archive-imports/" },
      "Expiration": { "Days": 1 }
    }
  ]
}
EOF
if ! mc ilm import "local/${BUCKET}" <"$lifecycle_tmp" >/dev/null 2>&1; then
  echo "minio-init: lifecycle import skipped (best-effort)" >&2
fi
rm -f "$lifecycle_tmp"

# Never set public anonymous download policy.
exit 0
