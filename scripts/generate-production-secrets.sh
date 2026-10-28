#!/usr/bin/env bash
# Fills the [GENERATED] keys of ~/.config/ghostpost/production.env without
# touching existing values, and creates the backup age keypair. Never prints
# secret values. Safe to re-run.
set -euo pipefail

DIR="${GHOSTPOST_CONFIG_DIR:-$HOME/.config/ghostpost}"
ENV_FILE="$DIR/production.env"
AGE_KEY="$DIR/backup-age.key"

command -v openssl >/dev/null || { echo "openssl required" >&2; exit 1; }
command -v age-keygen >/dev/null || { echo "age-keygen required (brew install age)" >&2; exit 1; }

umask 077
mkdir -p "$DIR"
chmod 700 "$DIR"
touch "$ENV_FILE"
chmod 600 "$ENV_FILE"

has() { grep -Eq "^$1=.+" "$ENV_FILE"; }
set_if_missing() {
  local key="$1" value="$2"
  if has "$key"; then echo "kept      $key"; return; fi
  # Drop an empty placeholder line, then append.
  local tmp; tmp="$(mktemp)"
  grep -Ev "^$key=\$" "$ENV_FILE" >"$tmp" || true
  printf '%s=%s\n' "$key" "$value" >>"$tmp"
  mv "$tmp" "$ENV_FILE"
  chmod 600 "$ENV_FILE"
  echo "generated $key"
}

b64_32() { openssl rand -base64 32 | tr -d '\n'; }
pw() { openssl rand -hex 24; }
stamp="$(date -u +%Y%m%d)"

set_if_missing POSTGRES_SUPERUSER_PASSWORD "$(pw)"
set_if_missing POSTGRES_MIGRATOR_PASSWORD "$(pw)"
set_if_missing POSTGRES_APP_PASSWORD "$(pw)"
set_if_missing WORKOS_COOKIE_PASSWORD "$(openssl rand -hex 32)"
set_if_missing APP_SESSION_KEYS "[{\"id\":\"prod_$stamp\",\"secret\":\"$(b64_32)\"}]"
set_if_missing ARCHIVE_FINGERPRINT_KEYS "[{\"version\":1,\"secret\":\"$(b64_32)\"}]"
set_if_missing SCAN_BATCH_HMAC_KEYS "[{\"id\":\"prod_$stamp\",\"secret\":\"$(b64_32)\"}]"
set_if_missing SCAN_LLM_PROVIDER jev

if [[ ! -f "$AGE_KEY" ]]; then
  age-keygen -o "$AGE_KEY" 2>/dev/null
  chmod 600 "$AGE_KEY"
  echo "generated $AGE_KEY (backup decryption key: move a copy to your password manager)"
fi
set_if_missing BACKUP_AGE_RECIPIENT "$(age-keygen -y "$AGE_KEY")"

missing=()
while IFS= read -r key; do
  has "$key" || missing+=("$key")
done < <(sed -nE 's/^([A-Z0-9_]+)=.*/\1/p' "$(cd "$(dirname "$0")/.." && pwd)/.env.production.example")
if ((${#missing[@]})); then
  printf 'still missing (fill these in %s):\n' "$ENV_FILE"
  printf '  %s\n' "${missing[@]}"
else
  echo "all production keys present"
fi
