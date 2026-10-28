#!/usr/bin/env bash
# Deploy a committed SHA to the ghostpost VM (ssh alias `ghostpost-vm`).
#   scripts/deploy-htpc.sh <git-sha|HEAD>
#   scripts/deploy-htpc.sh --worktree        snapshot of the working tree (tracked +
#                                            untracked, minus .gitignore) as an
#                                            unreferenced commit; no refs change
#   scripts/deploy-htpc.sh --rollback <previously deployed sha>
# Ships `git archive <sha>` (never the working tree) plus the allowlisted keys
# of ~/.config/ghostpost/production.env, builds natively on the VM, migrates,
# then rolls the API. Migrations must stay expand-only so rollback is safe.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VM="${GHOSTPOST_VM:-ghostpost-vm}"
ENV_SRC="${GHOSTPOST_ENV_FILE:-$HOME/.config/ghostpost/production.env}"
APP=/opt/ghostpost
API_URL="${GHOSTPOST_API_URL:-https://api.getghostpost.com}"

die() { echo "deploy: $*" >&2; exit 1; }

rollback=0
if [[ "${1:-}" == "--rollback" ]]; then rollback=1; shift; fi
if [[ "${1:-}" == "--worktree" ]]; then
  idx="$(mktemp)"; rm -f "$idx"
  GIT_INDEX_FILE="$idx" git -C "$ROOT" add -A
  tree="$(GIT_INDEX_FILE="$idx" git -C "$ROOT" write-tree)"; rm -f "$idx"
  ref="$(git -C "$ROOT" commit-tree "$tree" -p HEAD -m "worktree snapshot $(date -u +%FT%TZ)")"
  echo "==> worktree snapshot $ref"
else
  ref="${1:?usage: deploy-htpc.sh <sha> | --worktree | --rollback <sha>}"
fi
sha="$(git -C "$ROOT" rev-parse --verify "${ref}^{commit}" 2>/dev/null)" || die "unknown commit $ref"
short="${sha:0:12}"

[[ -f "$ENV_SRC" ]] || die "missing $ENV_SRC (run scripts/generate-production-secrets.sh)"
[[ "$(stat -f %Lp "$ENV_SRC" 2>/dev/null || stat -c %a "$ENV_SRC")" == 600 ]] || die "$ENV_SRC must be mode 600"

# Allowlisted env file: only keys named in .env.production.example, all non-empty.
env_tmp="$(mktemp)"; trap 'rm -f "$env_tmp"' EXIT
missing=()
while IFS= read -r key; do
  line="$(grep -E "^${key}=" "$ENV_SRC" | tail -1 || true)"
  [[ -n "${line#*=}" ]] || { missing+=("$key"); continue; }
  printf '%s\n' "$line" >>"$env_tmp"
done < <(git -C "$ROOT" show "$sha:.env.production.example" | sed -nE 's/^([A-Z0-9_]+)=.*/\1/p')
((${#missing[@]} == 0)) || die "missing values in $ENV_SRC: ${missing[*]}"
printf 'GHOSTPOST_RELEASE=%s\n' "$short" >>"$env_tmp"

if ((rollback)); then
  ssh "$VM" "test -d $APP/releases/$short" || die "release $short not on the VM"
else
  echo "==> ship $short"
  ssh "$VM" "mkdir -p $APP/releases/$short.tmp && rm -rf $APP/releases/$short.tmp/*"
  git -C "$ROOT" archive --format=tar "$sha" | ssh "$VM" "tar -x -C $APP/releases/$short.tmp && rm -rf $APP/releases/$short && mv $APP/releases/$short.tmp $APP/releases/$short"
fi
ssh "$VM" "umask 077 && cat > $APP/.env.production.new && mv $APP/.env.production.new $APP/.env.production" <"$env_tmp"

echo "==> build, migrate, roll ($short)"
ssh "$VM" bash -s -- "$short" "$rollback" <<'REMOTE'
set -euo pipefail
short="$1" rollback="$2"
APP=/opt/ghostpost
exec 9>"$APP/.deploy.lock"
flock -n 9 || { echo "another deploy is running" >&2; exit 1; }
cd "$APP/releases/$short"
dc() { docker compose -f docker-compose.prod.yml --env-file "$APP/.env.production" "$@"; }
if [[ "$rollback" == 0 ]]; then dc build --pull migrate backup; fi
dc up -d --wait postgres
dc run --rm --interactive=false migrate
dc up -d --wait --remove-orphans api
dc up -d cloudflared backup
ln -sfn "$APP/releases/$short" "$APP/current.new" && mv -T "$APP/current.new" "$APP/current"
printf '{"release":"%s","rollback":%s,"deployedAt":"%s"}\n' "$short" "$rollback" "$(date -u +%FT%TZ)" >>"$APP/deploys.jsonl"
dc ps --format '{{.Service}} {{.Status}}'
# Keep the 5 newest releases (never the current one).
ls -1dt "$APP"/releases/*/ | tail -n +6 | while read -r old; do
  [[ "$(readlink -f "$APP/current")/" == "$old" ]] || rm -rf "$old"
done
REMOTE

echo "==> smoke $API_URL"
if [[ -n "$(dig +short "${API_URL#https://}" 2>/dev/null)" ]]; then
  for _ in $(seq 1 12); do
    curl -fsS -m 10 "$API_URL/health/ready" >/dev/null 2>&1 && break
    sleep 5
  done
  bash "$ROOT/scripts/smoke-health.sh" "$API_URL"
  bash "$ROOT/scripts/smoke-workos-authorize.sh" "$API_URL"
else
  echo "deploy: ${API_URL#https://} does not resolve yet; skipped public smoke"
fi
echo "deploy: $short live"
