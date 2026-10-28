#!/bin/sh
# Production DDL/DML login bootstrap. Runs once, on first volume init, as the
# superuser inside the postgres container. Passwords come from the environment.
set -eu
: "${POSTGRES_MIGRATOR_PASSWORD:?required}"
: "${POSTGRES_APP_PASSWORD:?required}"

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  -v migrator_pw="$POSTGRES_MIGRATOR_PASSWORD" -v app_pw="$POSTGRES_APP_PASSWORD" <<'SQL'
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS citext;
CREATE ROLE ghostpost_migrator LOGIN PASSWORD :'migrator_pw';
CREATE ROLE ghostpost_app LOGIN PASSWORD :'app_pw';
GRANT CONNECT, CREATE ON DATABASE ghostpost TO ghostpost_migrator;
GRANT USAGE, CREATE ON SCHEMA public TO ghostpost_migrator;
GRANT CONNECT ON DATABASE ghostpost TO ghostpost_app;
GRANT USAGE ON SCHEMA public TO ghostpost_app;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
SQL
