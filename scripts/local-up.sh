#!/usr/bin/env bash
# Ordered local stack start for Ghostpost (Plan 008).
# No secrets in this script — copy .env.example → .env first.
#
# Device notes (comments only):
# - Android emulator API base: http://10.0.2.2:8080
# - Physical device: API_PUBLISH_HOST=0.0.0.0 and EXPO_PUBLIC_API_URL=http://<LAN-IP>:8080
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ ! -f .env ]]; then
  echo "missing .env — copy .env.example to .env and fill WorkOS / fingerprint secrets" >&2
  exit 1
fi

echo "==> postgres + minio"
docker compose up -d --wait postgres minio

echo "==> minio-init (bucket + versioning)"
docker compose run --rm minio-init

echo "==> migrate"
docker compose run --rm migrate

echo "==> backend"
docker compose up -d --wait --no-deps backend

echo
echo "Stack is up."
echo "  live:  curl -fsS http://127.0.0.1:8080/health/live"
echo "  ready: curl -fsS http://127.0.0.1:8080/health/ready"
echo
echo "Expo against this API:"
echo "  EXPO_PUBLIC_API_URL=http://localhost:8080 npm --prefix app start"
echo
echo "Durable upload smoke (needs GP_TEST_SESSION after WorkOS exchange):"
echo "  bash scripts/local-upload-smoke.sh"
