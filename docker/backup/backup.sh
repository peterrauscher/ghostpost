#!/bin/sh
# pg_dump -> age (public-key encryption) -> R2 (S3 SigV4 via curl).
# The dump is encrypted in the pipe; only ciphertext ever touches disk.
#   ghostpost-backup once   one backup now
#   ghostpost-backup loop   daily at BACKUP_HOUR_UTC (default 03)
set -eu

: "${PGHOST:?}" "${PGUSER:?}" "${PGPASSWORD:?}" "${PGDATABASE:?}"
: "${BACKUP_AGE_RECIPIENT:?}" "${R2_ACCOUNT_ID:?}" "${BACKUP_BUCKET:?}"
: "${R2_BACKUP_ACCESS_KEY_ID:?}" "${R2_BACKUP_SECRET_ACCESS_KEY:?}"
SPOOL="${BACKUP_SPOOL_DIR:-/var/spool/ghostpost-backup}"
ENDPOINT="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com"

log() { printf '{"ts":"%s","event":"%s"%s}\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1" "${2:-}"; }
once() {
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  key="postgres/$(date -u +%Y/%m/%d)/${stamp}.dump.age"
  out="$SPOOL/${stamp}.dump.age"
  mkdir -p "$SPOOL"
  status=0
  upload "$out" "$key" || status=$?
  rm -f "$out"
  return "$status"
}

upload() {
  out="$1" key="$2"
  # pipefail is not POSIX: check pg_dump's exit through a status file.
  { pg_dump --format=custom --no-owner --no-privileges || echo $? >"$out.pgfail"; } | age -r "$BACKUP_AGE_RECIPIENT" >"$out"
  if [ -f "$out.pgfail" ]; then rm -f "$out.pgfail"; log backup_failed ',"reason":"pg_dump"'; return 1; fi
  size="$(wc -c <"$out" | tr -d ' ')"
  [ "$size" -gt 0 ] || { log backup_failed ',"reason":"empty dump"'; return 1; }
  sha="$(sha256sum "$out" | cut -d' ' -f1)"
  if [ "${BACKUP_DRY_RUN:-0}" = 1 ]; then
    log backup_dry_run ",\"bytes\":$size,\"key\":\"$key\""
    return 0
  fi
  code="$(curl -sS -o /dev/null -w '%{http_code}' --retry 3 \
    --aws-sigv4 "aws:amz:auto:s3" --user "${R2_BACKUP_ACCESS_KEY_ID}:${R2_BACKUP_SECRET_ACCESS_KEY}" \
    -H "x-amz-content-sha256: $sha" -H "content-type: application/octet-stream" \
    -T "$out" "$ENDPOINT/$BACKUP_BUCKET/$key")"
  [ "$code" = 200 ] || { log backup_failed ",\"status\":$code"; return 1; }
  log backup_uploaded ",\"bytes\":$size,\"key\":\"$key\""
}
case "${1:-loop}" in
  once) once ;;
  loop)
    hour="${BACKUP_HOUR_UTC:-03}"
    while :; do
      now="$(date -u +%s)"
      next="$(date -u -d "today ${hour}:00" +%s)"
      [ "$next" -gt "$now" ] || next="$(date -u -d "tomorrow ${hour}:00" +%s)"
      sleep $((next - now))
      once || log backup_retry_tomorrow
    done ;;
  *) echo "usage: ghostpost-backup once|loop" >&2; exit 2 ;;
esac
