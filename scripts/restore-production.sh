#!/usr/bin/env bash
# Restore an age-encrypted production pg_dump into a FRESH, prepared VPS.
#
#   bash scripts/restore-production.sh --release <sha> --backup <file.dump.age> \
#        --restore-point <RFC3339> [--identity <age identity file>]
#
# Run after `bash scripts/deploy-vps.sh --prepare <sha>` of the same release.
# Environment: GHOSTPOST_HOST (default debian@40.160.254.201),
#              GHOSTPOST_ENV_FILE (default ~/.config/ghostpost/production.env).
#
# Safety contract
#   * The dump is decrypted LOCALLY and streamed over SSH into pg_restore. The age
#     identity and the plaintext dump never reach the server (no disk, no env).
#   * Only a database holding no application objects is restored into, and only
#     while nothing but postgres runs in the compose project. There is no
#     reset/drop/force option. pg_restore runs --single-transaction
#     --exit-on-error as ghostpost_migrator: all of it commits or none of it.
#   * Order: restore -> selected release `migrate` (refreshes app grants) -> seed
#     the restore guard with a random token (only its SHA-256 is stored) ->
#     `deletion replay` against the preserved R2 ledger -> sanity checks ->
#     atomic /opt/ghostpost/.database-restored. The API, workers, Caddy and the
#     backup service are never started here, so a failure can never go live.
#   * Failure after the restore commits leaves /opt/ghostpost/.restore-state. Re-
#     running the IDENTICAL command (same release, backup bytes, restore point)
#     resumes at migrate/replay without restoring again. Anything else is refused.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOST="${GHOSTPOST_HOST:-debian@40.160.254.201}"
ENV_SRC="${GHOSTPOST_ENV_FILE:-$HOME/.config/ghostpost/production.env}"
APP=/opt/ghostpost
LEDGER_RETENTION_SECS=$((21 * 86400))
SSH=(ssh -T -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=15 -o ServerAliveInterval=15 -o ServerAliveCountMax=8)

die() { echo "restore: $*" >&2; exit 1; }
log() { echo "==> $*" >&2; }
usage() {
  cat >&2 <<'USAGE'
usage: restore-production.sh --release <sha> --backup <local age-encrypted dump>
                             --restore-point <RFC3339> [--identity <local age key>]
USAGE
  exit 2
}

# --- arguments --------------------------------------------------------------
release="" backup="" restore_point_in="" identity="$HOME/.config/ghostpost/backup-age.key"
while (($#)); do
  case "$1" in
    --release) (($# >= 2)) || usage; release="$2"; shift 2 ;;
    --backup) (($# >= 2)) || usage; backup="$2"; shift 2 ;;
    --restore-point) (($# >= 2)) || usage; restore_point_in="$2"; shift 2 ;;
    --identity) (($# >= 2)) || usage; identity="$2"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "restore: unknown argument: $1" >&2; usage ;;
  esac
done
[[ -n "$release" && -n "$backup" && -n "$restore_point_in" ]] || usage

for tool in git ssh age shasum tar find xargs mktemp; do
  command -v "$tool" >/dev/null || die "missing local tool: $tool"
done

# --- release ----------------------------------------------------------------
re='^[0-9a-fA-F]{7,40}$'
[[ "$release" =~ $re ]] || die "--release must be a hex Git SHA"
sha="$(git -C "$ROOT" rev-parse --verify "${release}^{commit}" 2>/dev/null)" || die "unknown commit $release"
short="${sha:0:12}"

# --- restore point: strict RFC3339, normalised to UTC, bounded --------------
epoch_utc() {
  local y=$((10#$1)) m=$((10#$2)) d=$((10#$3)) h=$((10#$4)) mi=$((10#$5)) s=$((10#$6))
  local era yoe mp doy doe days
  if ((m <= 2)); then y=$((y - 1)); fi
  era=$((y / 400))
  yoe=$((y - era * 400))
  mp=$(((m + 9) % 12))
  doy=$(((153 * mp + 2) / 5 + d - 1))
  doe=$((yoe * 365 + yoe / 4 - yoe / 100 + doy))
  days=$((era * 146097 + doe - 719468))
  echo $((days * 86400 + h * 3600 + mi * 60 + s))
}
re='^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\.[0-9]{1,9})?(Z|[+-]([01][0-9]|2[0-3]):[0-5][0-9])$'
[[ "$restore_point_in" =~ $re ]] || die "--restore-point must be RFC3339, e.g. 2026-11-11T10:02:46Z"
rp_y="${restore_point_in:0:4}" rp_m="${restore_point_in:5:2}" rp_d="${restore_point_in:8:2}"
rp_h="${restore_point_in:11:2}" rp_mi="${restore_point_in:14:2}" rp_s="${restore_point_in:17:2}"
rp_tail="$(printf '%s' "${restore_point_in:19}" | sed -E 's/^\.[0-9]+//')"
days_in_month() { # year month
  local y=$((10#$1)) m=$((10#$2))
  case "$m" in
    1|3|5|7|8|10|12) echo 31 ;;
    4|6|9|11) echo 30 ;;
    2) if ((y % 4 == 0 && (y % 100 != 0 || y % 400 == 0))); then echo 29; else echo 28; fi ;;
  esac
}
((10#$rp_d <= $(days_in_month "$rp_y" "$rp_m"))) || die "--restore-point is not a real calendar date"
rp_off=0
if [[ "$rp_tail" != Z ]]; then
  rp_off=$((10#${rp_tail:1:2} * 3600 + 10#${rp_tail:4:2} * 60))
  if [[ "${rp_tail:0:1}" == "-" ]]; then rp_off=$((-rp_off)); fi
fi
rp_epoch=$(($(epoch_utc "$rp_y" "$rp_m" "$rp_d" "$rp_h" "$rp_mi" "$rp_s") - rp_off))
now_epoch="$(date -u +%s)"
((rp_epoch <= now_epoch + 300)) || die "--restore-point is in the future"
((now_epoch - rp_epoch <= LEDGER_RETENTION_SECS)) || die "--restore-point is older than the 21-day deletion-ledger retention; deletions since then cannot be replayed"
restore_point="$(date -u -r "$rp_epoch" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d "@$rp_epoch" +%Y-%m-%dT%H:%M:%SZ)"
[[ "$restore_point" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] || die "could not normalise restore point"

# A restore point later than the backup's own stamp would skip deletions made in between.
base="$(basename "$backup")"
re='([0-9]{8})T([0-9]{6})Z'
if [[ "$base" =~ $re ]]; then
  d8="${BASH_REMATCH[1]}" t6="${BASH_REMATCH[2]}"
  stamp_epoch="$(epoch_utc "${d8:0:4}" "${d8:4:2}" "${d8:6:2}" "${t6:0:2}" "${t6:2:2}" "${t6:4:2}")"
  ((rp_epoch <= stamp_epoch)) || die "--restore-point is later than the backup timestamp in its file name ($base); deletions between them would not be replayed"
else
  log "warning: backup file name carries no YYYYMMDDTHHMMSSZ stamp; cannot cross-check --restore-point. It must be at or before the moment the dump's snapshot began."
fi

# --- local inputs -----------------------------------------------------------
mode_of() { stat -f %Lp "$1" 2>/dev/null || stat -c %a "$1"; }
[[ -f "$backup" && -r "$backup" && -s "$backup" ]] || die "backup $backup is not a readable non-empty file"
[[ -f "$identity" && -r "$identity" ]] || die "age identity $identity is not a readable file"
case "$(mode_of "$identity")" in 600|400) ;; *) die "$identity must be mode 600 or 400" ;; esac
[[ -f "$ENV_SRC" ]] || die "missing $ENV_SRC"
[[ "$(mode_of "$ENV_SRC")" == 600 ]] || die "$ENV_SRC must be mode 600"

work="$(umask 077 && mktemp -d "${TMPDIR:-/tmp}/ghostpost-restore.XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Single read of the ciphertext: everything below (hash, verification, streaming)
# uses this private copy, so the bytes verified are the bytes restored. Ciphertext only.
cp "$backup" "$work/backup.age"
backup_sha="$(shasum -a 256 "$work/backup.age" | cut -d' ' -f1)"

# Allowlisted env values of the selected release must equal what --prepare shipped.
: >"$work/env-hashes"
missing=()
while IFS= read -r key; do
  line="$(grep -E "^${key}=" "$ENV_SRC" | tail -1 || true)"
  if [[ -z "${line#*=}" ]]; then missing+=("$key"); continue; fi
  printf '%s %s\n' "$key" "$(printf '%s\n' "$line" | shasum -a 256 | cut -d' ' -f1)" >>"$work/env-hashes"
done < <(git -C "$ROOT" show "$sha:.env.production.example" | sed -nE 's/^([A-Z0-9_]+)=.*/\1/p')
((${#missing[@]} == 0)) || die "missing values in $ENV_SRC: ${missing[*]}"
[[ -s "$work/env-hashes" ]] || die "release $short has no allowlisted env keys"

# Content hashes of the selected release, to prove the staged directory is that commit.
mkdir "$work/tree"
git -C "$ROOT" archive --format=tar "$sha" backend docker docker-compose.prod.yml .env.production.example | tar -x -C "$work/tree"
(cd "$work/tree" && find backend docker docker-compose.prod.yml .env.production.example -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256) >"$work/tree-hashes"
[[ -s "$work/tree-hashes" ]] || die "release $short has no files"
if grep -q '[^A-Za-z0-9._/ -]' "$work/tree-hashes"; then die "unexpected characters in release file names"; fi
migration_count="$(find "$work/tree/backend/migrations" -type f -name '*.sql' | wc -l | tr -d ' ')"
((migration_count > 0)) || die "release $short has no migrations"

# --- remote side ------------------------------------------------------------
# One script, several actions. Everything is defined in functions and `main` runs
# last, so a syntax error cannot leave half of the script executed.
IFS= read -r -d '' REMOTE_SCRIPT <<'REMOTE' || true
set -euo pipefail
APP=/opt/ghostpost
REL="$APP/releases/$short"
STATE="$APP/.restore-state"
MARKER="$APP/.database-restored"
export GHOSTPOST_RELEASE="$short"

log() { printf 'remote: %s\n' "$*" >&2; }
die() { printf 'remote: refused/failed: %s\n' "$*" >&2; exit 1; }

# stdin of this shell is the script itself: never let a child read it.
dc() { (cd "$REL" && docker compose -f docker-compose.prod.yml --env-file "$APP/.env.production" "$@") </dev/null; }
dcin() { (cd "$REL" && docker compose -f docker-compose.prod.yml --env-file "$APP/.env.production" "$@"); }

# Passwords come from the postgres container's own environment; none cross SSH.
PSQL_SU='PGPASSWORD="$POSTGRES_PASSWORD" exec psql -X -q -w -h 127.0.0.1 -U ghostpost -d ghostpost -v ON_ERROR_STOP=1 -tA "$@"'
PSQL_MIG='PGPASSWORD="$POSTGRES_MIGRATOR_PASSWORD" exec psql -X -q -w -h 127.0.0.1 -U ghostpost_migrator -d ghostpost -v ON_ERROR_STOP=1 -tA "$@"'
psql_su() { dc exec -T postgres sh -c "$PSQL_SU" psql "$@"; }
psql_su_in() { dcin exec -T postgres sh -c "$PSQL_SU" psql "$@"; }
psql_mig() { dc exec -T postgres sh -c "$PSQL_MIG" psql "$@"; }
psql_mig_in() { dcin exec -T postgres sh -c "$PSQL_MIG" psql "$@"; }

IFS= read -r -d '' EMPTY_SQL <<'SQL' || true
SELECT
  (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_toast' AND n.nspname !~ '^pg_temp'
      AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid AND d.deptype = 'e'))
+ (SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_toast' AND n.nspname !~ '^pg_temp'
      AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_proc'::regclass AND d.objid = p.oid AND d.deptype = 'e'))
+ (SELECT count(*) FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE t.typtype IN ('e','d','r','m') AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname !~ '^pg_toast' AND n.nspname !~ '^pg_temp'
      AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_type'::regclass AND d.objid = t.oid AND d.deptype = 'e'))
+ (SELECT count(*) FROM pg_namespace WHERE nspname NOT IN ('public','pg_catalog','information_schema') AND nspname !~ '^pg_')
+ (SELECT count(*) FROM pg_extension WHERE extname NOT IN ('plpgsql','pgcrypto','citext'));
SQL

IFS= read -r -d '' OWNERSHIP_SQL <<'SQL' || true
SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = 'public' AND c.relkind IN ('r','p','v','m','S','f')
  AND pg_get_userbyid(c.relowner) <> 'ghostpost_migrator'
  AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid AND d.deptype = 'e');
SQL

IFS= read -r -d '' GRANTS_SQL <<'SQL' || true
SELECT count(*) FROM pg_tables
WHERE schemaname = 'public' AND tablename <> 'ghostpost_restore_guard'
  AND NOT (has_table_privilege('ghostpost_app', format('%I.%I', schemaname, tablename), 'SELECT')
       AND has_table_privilege('ghostpost_app', format('%I.%I', schemaname, tablename), 'INSERT')
       AND has_table_privilege('ghostpost_app', format('%I.%I', schemaname, tablename), 'UPDATE')
       AND has_table_privilege('ghostpost_app', format('%I.%I', schemaname, tablename), 'DELETE'));
SQL

state_get() { [ -f "$STATE" ] || return 0; sed -n "s/^$1=//p" "$STATE" | tail -1; }
# Client sessions on the database other than ours. A just-finished pg_restore backend
# may need a moment to exit, so settle before trusting a non-zero count.
session_count() {
  local n i=0
  while :; do
    n="$(psql_su -c "SELECT count(*) FROM pg_stat_activity WHERE datname = 'ghostpost' AND pid <> pg_backend_pid() AND backend_type = 'client backend'")"
    if [ "$n" = 0 ] || [ "$i" -ge 10 ]; then echo "$n"; return 0; fi
    i=$((i + 1)); sleep 1
  done
}
state_write() { # stage
  local tmp="$STATE.tmp.$$" started
  started="$(state_get started_at)"; : "${started:=$(date -u +%Y-%m-%dT%H:%M:%SZ)}"
  ( umask 077
    printf 'release=%s\nrestore_point=%s\nbackup_sha256=%s\nstage=%s\nstarted_at=%s\n' \
      "$short" "$restore_point" "$backup_sha" "$1" "$started" >"$tmp" )
  sync
  mv -f "$tmp" "$STATE"
}
state_matches() {
  [ -f "$STATE" ] \
    && [ "$(state_get release)" = "$short" ] \
    && [ "$(state_get restore_point)" = "$restore_point" ] \
    && [ "$(state_get backup_sha256)" = "$backup_sha" ]
}

take_lock() {
  [ -d "$APP" ] || die "$APP does not exist; run deploy-vps.sh --prepare first"
  exec 9>"$APP/.deploy.lock"
  flock -n 9 || die "another deploy or restore is running"
}

check_release() {
  [ ! -e "$MARKER" ] || die "$MARKER already exists: this host was already restored"
  [ -d "$REL" ] && [ -f "$REL/docker-compose.prod.yml" ] || die "release $short is not staged in $REL; run deploy-vps.sh --prepare $short first"
  [ -f "$APP/.env.production" ] || die "$APP/.env.production is missing; run --prepare"
  grep -Fxq "GHOSTPOST_RELEASE=$short" "$APP/.env.production" || die "$APP/.env.production is not for release $short; re-run --prepare"
  local out key want line got bad=""
  if ! out="$(cd "$REL" && printf '%s\n' "$tree_hashes" | sha256sum -c --quiet - 2>&1)"; then
    die "staged release differs from the selected commit: $(printf '%s' "$out" | head -5 | tr '\n' ';')"
  fi
  [ "$(find "$REL/backend/migrations" -type f -name '*.sql' | wc -l | tr -d ' ')" = "$migration_count" ] || die "staged migrations differ from the selected commit"
  while read -r key want; do
    [ -n "$key" ] || continue
    line="$(grep -E "^${key}=" "$APP/.env.production" | tail -1 || true)"
    got="$(printf '%s\n' "$line" | sha256sum | cut -d' ' -f1)"
    [ "$got" = "$want" ] || bad="$bad $key"
  done <<EOF
$env_hashes
EOF
  [ -z "$bad" ] || die "remote .env.production differs from the local env file for:$bad; re-run --prepare"
  docker image inspect "ghostpost-backend:$short" >/dev/null 2>&1 || die "image ghostpost-backend:$short is missing; run --prepare"
}

check_runtime_stopped() {
  local out bad
  out="$(docker ps --filter label=com.docker.compose.project=ghostpost --format '{{.Label "com.docker.compose.service"}}')" || die "cannot list containers"
  bad="$(printf '%s\n' "$out" | grep -v -x -e postgres -e '' || true)"
  [ -z "$bad" ] || die "services other than postgres are running ($(printf '%s' "$bad" | sort -u | tr '\n' ' ')); stop them deliberately first, this tool never stops production"
}

check_postgres() {
  local cid st n
  cid="$(dc ps -q postgres)"
  [ -n "$cid" ] || die "postgres is not running; run --prepare"
  st="$(docker inspect -f '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{end}}' "$cid")"
  [ "$st" = "running healthy" ] || die "postgres is not healthy ($st)"
  n="$(psql_su -c "SELECT count(*) FROM pg_roles WHERE rolname IN ('ghostpost_migrator','ghostpost_app')")"
  [ "$n" = 2 ] || die "database roles ghostpost_migrator/ghostpost_app are not initialised"
  psql_mig -c 'SELECT 1' >/dev/null || die "ghostpost_migrator cannot log in"
  n="$(session_count)"
  [ "$n" = 0 ] || die "$n other session(s) are connected to the database"
}

common_checks() { take_lock; check_release; check_runtime_stopped; check_postgres; }

db_objects() {
  local n
  n="$(printf '%s\n' "$EMPTY_SQL" | psql_su_in)"
  [[ "$n" =~ ^[0-9]+$ ]] || die "could not count database objects"
  echo "$n"
}
has_table() { [ "$(psql_su -c "SELECT to_regclass('public.$1') IS NOT NULL")" = t ]; }

mode=""
decide_mode() {
  local n
  n="$(db_objects)"
  if [ "$n" = 0 ]; then
    mode=restore
  elif state_matches; then
    has_table users && has_table _sqlx_migrations || die "restore state matches but the database does not look like a restored Ghostpost database"
    mode=resume
  else
    die "database ghostpost already holds $n application object(s) and no matching in-progress restore of this release, backup and restore point; this tool never overwrites a populated database"
  fi
}

abort_restore() {
  take_lock
  local n
  n="$(session_count)"
  [ "$n" = 0 ] || die "$n session(s) still connected; pg_restore may still be running. Wait, then re-run the same command"
  n="$(db_objects)"
  if [ "$n" = 0 ]; then
    rm -f "$STATE"
    log "restore rolled back: database is empty again; nothing was promoted"
  else
    log "database holds restored objects although the transfer reported an error; state kept: re-run the identical command to resume"
  fi
}

finish_restore() {
  common_checks
  decide_mode
  [ "$mode" = resume ] || die "database is empty: the restore did not complete"
  state_write restored

  log "running migrations of release $short"
  dc run --rm --no-deps -T migrate >&2 || die "migrate failed"

  log "seeding restore guard"
  local token hash
  token="$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"
  [[ "$token" =~ ^[0-9a-f]{64}$ ]] || die "could not generate guard token"
  hash="$(printf '%s' "$token" | sha256sum | cut -d' ' -f1)"
  psql_mig_in -v "h=$hash" >/dev/null <<'SQL'
INSERT INTO ghostpost_restore_guard (singleton, token_hash, updated_at)
VALUES (true, decode(:'h', 'hex'), now())
ON CONFLICT (singleton) DO UPDATE SET token_hash = excluded.token_hash, updated_at = now();
SQL

  log "replaying deletion ledger after $restore_point (API and workers stay stopped)"
  local mig_pw
  mig_pw="$(dc exec -T postgres printenv POSTGRES_MIGRATOR_PASSWORD)"
  [[ "$mig_pw" =~ ^[A-Za-z0-9._~-]+$ ]] || die "migrator password is not URL-safe"
  export RESTORE_REPLAY_PENDING=true RESTORE_POINT="$restore_point" RESTORE_GUARD_TOKEN="$token"
  export RESTORE_DATABASE_URL="postgres://ghostpost_migrator:${mig_pw}@postgres:5432/ghostpost"
  # Runs the api service definition (preserved R2/WorkOS env) as a one-off; it does not start the service.
  dc run --rm --no-deps -T \
    -e RESTORE_REPLAY_PENDING -e RESTORE_POINT -e RESTORE_GUARD_TOKEN -e RESTORE_DATABASE_URL \
    api deletion replay >&2 || die "deletion replay failed; nothing was promoted"
  unset RESTORE_REPLAY_PENDING RESTORE_POINT RESTORE_GUARD_TOKEN RESTORE_DATABASE_URL token mig_pw

  log "sanity checks"
  local ok bad n
  ok="$(psql_mig -c 'SELECT count(*) FROM _sqlx_migrations WHERE success')"
  bad="$(psql_mig -c 'SELECT count(*) FROM _sqlx_migrations WHERE NOT success')"
  [ "$ok" = "$migration_count" ] && [ "$bad" = 0 ] || die "migration ledger mismatch (applied=$ok failed=$bad expected=$migration_count)"
  n="$(printf '%s\n' "$OWNERSHIP_SQL" | psql_mig_in)"
  [ "$n" = 0 ] || die "$n public relation(s) are not owned by ghostpost_migrator"
  n="$(printf '%s\n' "$GRANTS_SQL" | psql_mig_in)"
  [ "$n" = 0 ] || die "$n public table(s) lack full DML grants for ghostpost_app"
  [ "$(psql_mig -c "SELECT has_table_privilege('ghostpost_app', 'public.ghostpost_restore_guard', 'SELECT')")" = f ] || die "ghostpost_app can read the restore guard"
  [ "$(psql_mig -c 'SELECT count(*) FROM users')" -ge 0 ] || die "users table unreadable"
  psql_mig -c 'DELETE FROM ghostpost_restore_guard' >/dev/null
  check_runtime_stopped

  local tmp="$APP/.database-restored.tmp.$$"
  ( umask 022
    printf '{"release":"%s","restorePoint":"%s","restoredAt":"%s"}\n' \
      "$short" "$restore_point" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$tmp" )
  sync
  mv -T "$tmp" "$MARKER"
  rm -f "$STATE"
  log "restore complete; $MARKER written; nothing was started"
}

main() {
  case "$action" in
    preflight) common_checks; decide_mode; echo "RESTORE_MODE=$mode" ;;
    begin)
      common_checks; decide_mode
      [ "$mode" = restore ] || die "a restore is not needed (mode=$mode)"
      state_write restoring
      # Nothing else to prepare: the init-time pgcrypto/citext extensions stay; the dump's
      # CREATE EXTENSION IF NOT EXISTS is a no-op and --no-comments skips the only
      # statements (extension comments) that would need superuser ownership.
      echo "BEGIN_OK" ;;
    abort) abort_restore ;;
    finish) finish_restore ;;
    *) die "unknown action $action" ;;
  esac
}
main
REMOTE

remote_action() { # action
  local action="$1"
  {
    printf "action='%s'\nshort='%s'\nrestore_point='%s'\nbackup_sha='%s'\nmigration_count='%s'\n" \
      "$action" "$short" "$restore_point" "$backup_sha" "$migration_count"
    printf "env_hashes='%s'\n" "$(cat "$work/env-hashes")"
    printf "tree_hashes='%s'\n" "$(cat "$work/tree-hashes")"
    printf '%s\n' "$REMOTE_SCRIPT"
  } | "${SSH[@]}" "$HOST" bash -s
}

# --- 1. read-only preflight -------------------------------------------------
log "preflight on $HOST (release $short)"
if ! pre_out="$(remote_action preflight)"; then die "preflight refused or failed; nothing was changed"; fi
case "$pre_out" in
  *RESTORE_MODE=restore*) mode=restore ;;
  *RESTORE_MODE=resume*) mode=resume ;;
  *) die "preflight returned no mode" ;;
esac

if [[ "$mode" == restore ]]; then
  # --- 2. verify the ciphertext end to end and read the dump's table of contents ---
  # Streams plaintext into `pg_restore -l` (no DB connection, no disk). Success proves
  # the identity decrypts and authenticates the whole file and it is a pg_restore archive.
  log "verifying backup decryption and archive (nothing is written)"
  toc_cmd="exec docker compose -f $APP/releases/$short/docker-compose.prod.yml --env-file $APP/.env.production exec -T postgres sh -c 'pg_restore -l; rc=\$?; cat >/dev/null; exit \$rc'"
  set +e
  age -d -i "$identity" "$work/backup.age" | "${SSH[@]}" "$HOST" "GHOSTPOST_RELEASE=$short; export GHOSTPOST_RELEASE; $toc_cmd" >"$work/toc.txt"
  st=("${PIPESTATUS[@]}")
  set -e
  # 141 = age died of SIGPIPE because ssh/pg_restore ended first: the target is the cause.
  if ((st[0] != 0 && st[0] != 141)); then die "age decryption failed (wrong identity or corrupt backup); nothing was changed"; fi
  if ((st[0] != 0 || st[1] != 0)); then die "target could not read the backup as a pg_restore archive (age=${st[0]} ssh/pg_restore=${st[1]}); nothing was changed"; fi
  grep -Eq ' TABLE public users( |$)' "$work/toc.txt" || die "backup has no public.users table: not a Ghostpost database dump"
  grep -Eq ' TABLE public _sqlx_migrations( |$)' "$work/toc.txt" || die "backup has no public._sqlx_migrations table: not a Ghostpost database dump"

  # --- 3. begin: re-check under the lock, record state, drop init-time extensions ---
  log "beginning restore"
  if ! remote_action begin >/dev/null; then die "could not begin restore; nothing was restored"; fi

  # --- 4. stream decrypted dump into pg_restore (single transaction) ---
  # shellcheck disable=SC2016 # The password expands only inside the target Postgres container.
  pg_restore_cmd='PGPASSWORD="$POSTGRES_MIGRATOR_PASSWORD" exec pg_restore -h 127.0.0.1 -U ghostpost_migrator -w -d ghostpost --exit-on-error --single-transaction --no-owner --no-privileges --no-tablespaces --no-comments'
  restore_cmd="cd $APP/releases/$short && exec flock -n $APP/.deploy.lock env GHOSTPOST_RELEASE=$short docker compose -f docker-compose.prod.yml --env-file $APP/.env.production exec -T postgres sh -c '$pg_restore_cmd'"
  log "restoring (decrypting locally, streaming over SSH)"
  set +e
  age -d -i "$identity" "$work/backup.age" | "${SSH[@]}" "$HOST" "$restore_cmd" >&2
  st=("${PIPESTATUS[@]}")
  set -e
  if ((st[0] != 0 || st[1] != 0)); then
    echo "restore: transfer failed (age=${st[0]} ssh/pg_restore=${st[1]}); checking what the database holds" >&2
    remote_action abort || echo "restore: could not verify the target; re-run the identical command, preflight will report its state" >&2
    die "restore failed; nothing was promoted"
  fi
else
  log "database already holds a restore of this exact release, backup and restore point; resuming at migrate/replay"
fi

# --- 5. migrate, guard, replay, sanity, marker ---
if ! remote_action finish; then
  die "finish failed. The restored data is in the database but NOTHING was promoted (no marker, no runtime). Fix the cause, then re-run the identical command to resume."
fi

log "restore complete for release $short at restore point $restore_point"
log "next: DNS if needed, then bash scripts/deploy-vps.sh $short"
