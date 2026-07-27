-- Local-only DDL/DML login bootstrap for docker-compose Postgres.
-- Runs as POSTGRES_USER on first volume init only.
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS citext;

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'ghostpost_migrator') THEN
    CREATE ROLE ghostpost_migrator LOGIN PASSWORD 'ghostpost_migrator';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'ghostpost_app') THEN
    CREATE ROLE ghostpost_app LOGIN PASSWORD 'ghostpost_app';
  END IF;
END
$$;

GRANT CONNECT, CREATE ON DATABASE ghostpost TO ghostpost_migrator;
GRANT USAGE, CREATE ON SCHEMA public TO ghostpost_migrator;
GRANT CONNECT ON DATABASE ghostpost TO ghostpost_app;
GRANT USAGE ON SCHEMA public TO ghostpost_app;
