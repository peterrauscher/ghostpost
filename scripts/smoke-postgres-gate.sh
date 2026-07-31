#!/bin/sh
set -eu
: "${DATABASE_URL_MIGRATOR:?required}"
: "${DATABASE_URL_APP:?required}"
if command -v psql >/dev/null 2>&1; then
  run_psql() { psql "$@"; }
elif command -v docker >/dev/null 2>&1; then
  run_psql() { docker compose exec -T postgres psql "$@"; }
else
  echo "psql or local Docker Compose is required" >&2; exit 127
fi
run_psql "$DATABASE_URL_MIGRATOR" -v ON_ERROR_STOP=1 -c 'SELECT version();' >/dev/null
run_psql "$DATABASE_URL_MIGRATOR" -v ON_ERROR_STOP=1 -c 'CREATE TABLE _ghostpost_gate(id int); DROP TABLE _ghostpost_gate;' >/dev/null
run_psql "$DATABASE_URL_APP" -v ON_ERROR_STOP=1 -c 'SELECT 1;' >/dev/null
if run_psql "$DATABASE_URL_APP" -v ON_ERROR_STOP=1 -c 'CREATE TABLE _ghostpost_gate_app(id int)' >/dev/null 2>&1; then
  run_psql "$DATABASE_URL_APP" -c 'DROP TABLE IF EXISTS _ghostpost_gate_app' >/dev/null 2>&1 || true
  echo 'application role unexpectedly has CREATE privilege' >&2
  exit 1
fi
printf 'Postgres vendor gate passed\n'
