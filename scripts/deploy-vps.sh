#!/usr/bin/env bash
# Deploy an immutable Git SHA of the Ghostpost backend/worker + PostgreSQL to the
# OVH VPS. Frontends (Cloudflare Pages/Workers), DNS, D1 and R2 are never touched.
#
#   scripts/deploy-vps.sh --prepare <sha>   ship release + secrets, build images,
#                                           start healthy Postgres ONLY. No
#                                           migrations, API, workers, Caddy or
#                                           backups; `current` is left untouched;
#                                           no public smoke. Used before
#                                           scripts/restore-production.sh.
#   scripts/deploy-vps.sh <sha>             migrate, start api + caddy + backup,
#                                           smoke the requested API URL, then
#                                           switch `current`. REFUSES unless the
#                                           restore marker /opt/ghostpost/.database-restored
#                                           exists AND is valid JSON {release,restorePoint,
#                                           restoredAt} as written by the restore tool
#                                           (validated again before `current` switches).
#                                           For the default production URL the TLS
#                                           connection must terminate on the target VPS
#                                           (curl remote_ip vs `ip -o address show scope
#                                           global` over ssh); a stale-DNS/Cloudflare
#                                           answer fails and `current` is not switched.
#   scripts/deploy-vps.sh --rollback <sha>  restart an already-deployed, retained
#                                           release against the CURRENT schema.
#                                           No migrations, no schema downgrade, no
#                                           secret/data overwrite, no old-host start.
#
# Environment:
#   GHOSTPOST_HOST      ssh target (required; user@host)
#   GHOSTPOST_ENV_FILE  local secrets (default ~/.config/ghostpost/production.env, mode 600)
#   GHOSTPOST_API_URL   URL the smoke tests hit (default https://api.getghostpost.com).
#                       Override only for deliberate isolated verification; the
#                       override is recorded in the deploy log.
#
# Only `git archive <sha>` is shipped (never the working tree; there is no
# --worktree mode). Secrets are the allowlisted keys of .env.production.example
# at <sha>, copied to /opt/ghostpost/.env.production (mode 600). `current` and
# the deploy log are only updated after the new release is ready AND the smoke
# tests pass. On any failed check the script exits non-zero and does NOT switch
# `current`; it never rolls back automatically, resets data, or starts any other
# host. Services already started by the failed run keep running; fix the cause
# and re-run (every step is idempotent), or run --rollback <known-good sha>.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOST="${GHOSTPOST_HOST:?GHOSTPOST_HOST must be set to the SSH target}"
ENV_SRC="${GHOSTPOST_ENV_FILE:-$HOME/.config/ghostpost/production.env}"
API_URL="${GHOSTPOST_API_URL:-https://api.getghostpost.com}"
APP=/opt/ghostpost

die() { echo "deploy: $*" >&2; exit 1; }
usage() { die "usage: deploy-vps.sh --prepare <sha> | <sha> | --rollback <sha>"; }

mode=deploy
case "${1:-}" in
  --prepare) mode=prepare; shift ;;
  --rollback) mode=rollback; shift ;;
  -*) usage ;;
esac
(($# == 1)) || usage
ref="$1"

[[ -n "$HOST" && "$HOST" != -* ]] || die "invalid GHOSTPOST_HOST"
api_url_pattern='^https?://([A-Za-z0-9._-]+|\[[A-Fa-f0-9:]+\])(:[0-9]+)?(/[A-Za-z0-9._:/-]*)?$'
[[ "$API_URL" =~ $api_url_pattern ]] || die "invalid GHOSTPOST_API_URL: $API_URL"
API_URL="${API_URL%/}"

SSH=(ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=15 -o ServerAliveInterval=15 -o ServerAliveCountMax=8)
rsh() { "${SSH[@]}" -- "$HOST" "$@"; }

sha="$(git -C "$ROOT" rev-parse --verify "${ref}^{commit}" 2>/dev/null)" || die "unknown commit $ref"
short="${sha:0:12}"

# Release must already be the post-tunnel stack: Caddy in, cloudflared out.
git -C "$ROOT" cat-file -e "$sha:docker/Caddyfile" 2>/dev/null || die "$short has no docker/Caddyfile (not a VPS release)"
compose_src="$(git -C "$ROOT" show "$sha:docker-compose.prod.yml")"
grep -qE '^  caddy:' <<<"$compose_src" || die "$short docker-compose.prod.yml has no caddy service"
! grep -qE '^  cloudflared:' <<<"$compose_src" || die "$short still defines cloudflared; refusing to ship the tunnel"

env_tmp=""
run=""
cleanup() {
  local rc=$?
  [[ -z "$env_tmp" ]] || rm -f "$env_tmp"
  if [[ -n "$run" ]]; then rsh "rm -rf $APP/incoming/$run" >/dev/null 2>&1 || true; fi
  exit "$rc"
}
trap cleanup EXIT

if [[ "$mode" != rollback ]]; then
  [[ -f "$ENV_SRC" ]] || die "missing $ENV_SRC (run scripts/generate-production-secrets.sh)"
  [[ "$(stat -f %Lp "$ENV_SRC" 2>/dev/null || stat -c %a "$ENV_SRC")" == 600 ]] || die "$ENV_SRC must be mode 600"

  # Allowlist: only keys named in .env.production.example at <sha>, all non-empty.
  env_tmp="$(mktemp)"
  chmod 600 "$env_tmp"
  missing=()
  while IFS= read -r key; do
    [[ "$key" != CLOUDFLARE_TUNNEL_TOKEN ]] || die "$short still allowlists CLOUDFLARE_TUNNEL_TOKEN; refusing to ship it"
    line="$(grep -E "^${key}=" "$ENV_SRC" | tail -1 || true)"
    [[ -n "${line#*=}" ]] || { missing+=("$key"); continue; }
    printf '%s\n' "$line" >>"$env_tmp"
  done < <(git -C "$ROOT" show "$sha:.env.production.example" | sed -nE 's/^([A-Z0-9_]+)=.*/\1/p')
  ((${#missing[@]} == 0)) || die "missing values in $ENV_SRC: ${missing[*]}"
  printf 'GHOSTPOST_RELEASE=%s\n' "$short" >>"$env_tmp"
fi

echo "==> $mode $short on $HOST"
rsh "test -d $APP && test -w $APP" || die "$APP missing or not writable on $HOST (host bootstrap not done, or ssh not trusted yet)"

# Public-origin proof (default production URL only): the target VPS's own global
# addresses, read over authenticated SSH. An explicit GHOSTPOST_API_URL override is
# deliberately isolated and skips this proof (and is reported as NOT production).
is_prod=0
target_ips=""
if [[ "$mode" != prepare && "$API_URL" == "https://api.getghostpost.com" ]]; then
  is_prod=1
  target_ips="$(rsh "ip -o address show scope global" | awk '$3 == "inet" || $3 == "inet6" { sub(/\/.*/, "", $4); print tolower($4) }')" \
    || die "could not read global addresses of $HOST over ssh"
  [[ -n "$target_ips" ]] || die "$HOST reports no global IP addresses; cannot verify $API_URL reaches it"
fi

if [[ "$mode" != rollback ]]; then
  run="$(date -u +%Y%m%dT%H%M%SZ)-$$"
  rsh "umask 077 && mkdir -p $APP/incoming/$run"
  if [[ "$(rsh "cat $APP/releases/$short/.git-sha 2>/dev/null || true")" == "$sha" ]]; then
    echo "==> release $short already shipped"
  else
    echo "==> ship $short"
    git -C "$ROOT" archive --format=tar "$sha" | rsh "mkdir -p $APP/incoming/$run/release && tar -x --no-same-owner -C $APP/incoming/$run/release"
  fi
  rsh "umask 077 && cat > $APP/incoming/$run/env && chmod 600 $APP/incoming/$run/env" <"$env_tmp"
fi

# The script travels base64-encoded as `bash -c` text so stdin stays free (docker
# exec/run must not swallow it); every argument is %q-quoted for the remote shell.
# Empty args are passed as "-".
remote() {
  local b64; b64="$(printf '%s' "$REMOTE_SCRIPT" | base64 | tr -d '\n')"
  rsh "bash -c \"\$(echo $b64 | base64 -d)\" remote $(printf '%q ' "$@")" </dev/null
}

# Remote step. Runs under the deploy lock. phase=apply does everything up to and
# including readiness; phase=finalize (after the public smoke) switches `current`.
# shellcheck disable=SC2016
REMOTE_SCRIPT='
set -euo pipefail
phase="$1" mode="$2" short="$3" sha="$4" run="$5" url="$6"
APP=/opt/ghostpost
die() { echo "remote: $*" >&2; exit 1; }
exec 9>"$APP/.deploy.lock"
flock -n 9 || die "another deploy is running"
command -v docker >/dev/null && docker compose version >/dev/null || die "docker compose not available"
mkdir -p "$APP/releases" "$APP/incoming"
rel="$APP/releases/$short"

if [[ "$phase" == apply && "$run" != - ]]; then
  # Install the shipped release (atomic rename, never replace an existing one).
  inc="$APP/incoming/$run"
  if [[ -d "$inc/release" ]]; then
    [[ -f "$inc/release/docker-compose.prod.yml" && -f "$inc/release/docker/Caddyfile" ]] || die "shipped release is incomplete"
    if [[ -e "$rel" ]]; then
      [[ "$(cat "$rel/.git-sha" 2>/dev/null || true)" == "$sha" ]] || die "$rel exists with different content"
    else
      printf "%s" "$sha" >"$inc/release/.git-sha"
      mv -T "$inc/release" "$rel"
    fi
  fi
fi
[[ -f "$rel/.git-sha" && "$(cat "$rel/.git-sha")" == "$sha" ]] || die "release $short not installed on this host"

marker="$APP/.database-restored"
# Exactly what scripts/restore-production.sh writes: {"release","restorePoint","restoredAt"}.
# release is the 12-char short SHA the restore tool stages (a full 40-char SHA is also
# accepted); both timestamps are UTC RFC3339 (YYYY-MM-DDTHH:MM:SSZ), valid calendar
# instants, restorePoint <= restoredAt. Provenance only: the restored release need not
# equal any later deployed release. Contents are never echoed.
marker_filter="$(cat <<"JQ"
def ts: type == "string"
  and test("^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]Z$")
  and ((try fromdateiso8601 catch null) as $e | $e != null and ($e | todate) == .);
length == 1 and (.[0]
  | type == "object"
  and (keys == ["release", "restorePoint", "restoredAt"])
  and (.release | type == "string" and test("^([0-9a-f]{12}|[0-9a-f]{40})$"))
  and (.restorePoint | ts)
  and (.restoredAt | ts)
  and (.restorePoint <= .restoredAt))
JQ
)"
validate_marker() {
  [[ -f "$marker" && ! -L "$marker" ]] || die "$marker missing: database not restored on this host. Run scripts/restore-production.sh first"
  command -v jq >/dev/null || die "jq missing on this host (rerun host bootstrap); cannot validate $marker"
  jq -es -e "$marker_filter" "$marker" >/dev/null 2>&1 || die "$marker is not a valid restore marker (expected {release,restorePoint,restoredAt} as written by scripts/restore-production.sh)"
}
dc() { docker compose -f docker-compose.prod.yml --env-file "$APP/.env.production" "$@"; }
cd "$rel"
export GHOSTPOST_RELEASE="$short"

keep_releases() {
  # Keep the 5 newest releases, the current one and this one.
  local cur old
  cur="$(readlink -f "$APP/current" 2>/dev/null || true)"
  ls -1dt "$APP"/releases/*/ | tail -n +6 | while read -r old; do
    old="${old%/}"
    [[ "$old" != "$cur" && "$old" != "$rel" ]] || continue
    rm -rf "$old"
    docker image rm "ghostpost-backend:$(basename "$old")" "ghostpost-backup:$(basename "$old")" >/dev/null 2>&1 || true
  done
  find "$APP/incoming" -mindepth 1 -maxdepth 1 -mmin +1440 -exec rm -rf {} + 2>/dev/null || true
}
log_deploy() {
  printf "{\"release\":\"%s\",\"mode\":\"%s\",\"sha\":\"%s\",\"verified\":\"%s\",\"at\":\"%s\"}\n" \
    "$short" "$1" "$sha" "$2" "$(date -u +%FT%TZ)" >>"$APP/deploys.jsonl"
}
install_env() {
  [[ -f "$APP/incoming/$run/env" ]] || die "no shipped env for $short"
  install -m 600 "$APP/incoming/$run/env" "$APP/.env.production.new"
  mv -f "$APP/.env.production.new" "$APP/.env.production"
}
[[ -f "$APP/.env.production" || "$phase$mode" == applyprepare || "$phase$mode" == applydeploy ]] || die "missing $APP/.env.production"

if [[ "$phase" == finalize ]]; then
  validate_marker
  cid="$(dc ps -q api)"
  [[ -n "$cid" ]] || die "api is not running; not switching current"
  [[ "$(docker inspect -f "{{.Config.Image}}" "$cid")" == "ghostpost-backend:$short" ]] || die "api is not running release $short; not switching current"
  dc exec -T api curl -fsS http://127.0.0.1:8080/health/ready >/dev/null || die "api not ready; not switching current"
  ln -sfn "$rel" "$APP/current.new" && mv -T "$APP/current.new" "$APP/current"
  log_deploy "$mode" "$url"
  keep_releases
  dc ps --format "{{.Service}} {{.Status}}"
  exit 0
fi

# ---- phase apply ----------------------------------------------------------
if [[ "$mode" == prepare ]]; then
  [[ ! -e "$marker" ]] || die "$marker exists: this host is already restored; use a normal deploy"
  busy="$(docker ps --filter label=com.docker.compose.project=ghostpost --format "{{.Label \"com.docker.compose.service\"}}" | grep -E "^(api|caddy|backup|migrate|cloudflared)$" || true)"
  [[ -z "$busy" ]] || die "refusing to prepare while services are running: $(echo $busy)"
else
  validate_marker
fi

if [[ "$mode" != rollback ]]; then install_env; fi

dc config -q || die "docker compose config invalid for $short"
svcs="$(dc config --services)"
grep -qx caddy <<<"$svcs" || die "release $short has no caddy service"
! grep -qx cloudflared <<<"$svcs" || die "release $short still defines cloudflared"

if [[ "$mode" == rollback ]]; then
  grep -Eq "\"release\":\"$short\",\"mode\":\"(deploy|rollback)\"" "$APP/deploys.jsonl" 2>/dev/null || die "$short was never successfully deployed on this host; rollback only targets known releases"
  docker image inspect "ghostpost-backend:$short" "ghostpost-backup:$short" >/dev/null 2>&1 || die "images for $short are no longer on this host; rollback unavailable"
else
  echo "==> build ($short)"
  dc build --pull migrate backup
  if [[ "$mode" == deploy ]]; then dc pull --quiet caddy; fi
fi

echo "==> postgres"
dc up -d --wait --no-deps postgres

if [[ "$mode" == prepare ]]; then
  log_deploy prepare none
  keep_releases
  dc ps --format "{{.Service}} {{.Status}}"
  echo "prepared $short: Postgres only; no migrations/API/Caddy/backup; current unchanged"
  exit 0
fi

if [[ "$mode" == rollback ]]; then
  applied="$(dc exec -T postgres psql -U ghostpost -d ghostpost -Atc "select version from _sqlx_migrations where success" | sort)"
  known="$(ls backend/migrations/*.sql | sed -E "s#.*/([0-9]+)_.*#\1#" | sort)"
  unapplied="$(comm -23 <(printf "%s\n" "$known") <(printf "%s\n" "$applied"))"
  [[ -z "$unapplied" ]] || die "release $short needs migrations missing from the database ($(echo $unapplied)); incompatible, not rolling back"
else
  echo "==> stop API and worker before migration"
  dc stop --timeout 45 api
  echo "==> migrate"
  dc run --rm --interactive=false migrate
fi

echo "==> api, caddy, backup"
dc up -d --wait --no-deps --remove-orphans api
dc exec -T api curl -fsS http://127.0.0.1:8080/health/ready >/dev/null || die "api not ready"
dc up -d --wait --no-deps caddy backup
dc ps --format "{{.Service}} {{.Status}}"
'

case "$mode" in
  prepare)
    remote apply prepare "$short" "$sha" "$run" none
    echo "deploy: $short prepared on $HOST (Postgres only). Nothing is live; DNS and current unchanged."
    exit 0
    ;;
esac

remote apply "$mode" "$short" "$sha" "${run:--}" none

# Public-origin proof: the TLS connection's remote_ip must be one of the target VPS's addresses. Sets origin_ip.
probe_origin() {
  local out
  out="$(curl -fsS -m 10 -o /dev/null -w '%{remote_ip}' "$API_URL/health/ready" 2>/dev/null)" || return 1
  origin_ip="$(printf '%s' "$out" | tr '[:upper:]' '[:lower:]')"
  origin_ip="${origin_ip#::ffff:}"
  [[ -n "$origin_ip" ]]
}
origin_matches() { grep -Fxq -- "$origin_ip" <<<"$target_ips"; }

echo "==> smoke $API_URL"
ready=0
origin_ip=""
for _ in $(seq 1 36); do
  if probe_origin; then
    if ((!is_prod)) || origin_matches; then ready=1; break; fi
  fi
  sleep 5
done
if ((!ready)); then
  if [[ -n "$origin_ip" ]] && ((is_prod)); then
    die "$API_URL answers from $origin_ip, which is not $HOST's address (${target_ips//$'\n'/ }): DNS still points at the old host or a Cloudflare edge/tunnel. $short is running on $HOST but NOT verified and NOT marked current; fix the DNS-only A/AAAA record and re-run (nothing was rolled back)"
  fi
  curl -fsS -m 10 "$API_URL/health/ready" >&2 || true
  die "$API_URL never became ready. $short is running on $HOST but NOT verified and NOT marked current (check DNS/TLS for the hostname, then re-run; nothing was rolled back)"
fi
bash "$ROOT/scripts/smoke-health.sh" "$API_URL" || die "health smoke failed at $API_URL; $short NOT marked current"
bash "$ROOT/scripts/smoke-workos-authorize.sh" "$API_URL" || die "WorkOS authorize smoke failed at $API_URL; $short NOT marked current"
# The smoke scripts open their own connections; confirm DNS did not move meanwhile.
if ((is_prod)); then
  if ! probe_origin || ! origin_matches; then
    die "$API_URL no longer reaches $HOST after smoke (saw ${origin_ip:-no connection}); $short NOT marked current"
  fi
fi

remote finalize "$mode" "$short" "$sha" - "$API_URL"
if ((is_prod)); then
  echo "deploy: $short live at $API_URL"
else
  echo "deploy: $short verified at override URL $API_URL (NOT the production hostname) and marked current"
fi
