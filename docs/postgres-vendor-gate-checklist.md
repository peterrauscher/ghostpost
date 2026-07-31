# Production Postgres gate: Neon

Deployment remains blocked until every item is checked and evidence is attached to release record.

- [ ] Neon project uses PostgreSQL 17 compatibility.
- [ ] Direct TLS URLs use `sslmode=verify-full`; migrator and ECS runtime do not use the pooled hostname.
- [ ] Compute is reachable from ECS public subnets without an IP allowlist. If an allowlist or private endpoint becomes mandatory, stop and add NAT with stable EIPs or approved private connectivity.
- [ ] Separate `ghostpost_migrator` and `ghostpost_app` roles exist. Migrator can run DDL and grants; app has table/sequence DML and no schema `CREATE`.
- [ ] Branch restore/PITR drill passed. Record Neon's actual restore window as `POSTGRES_PITR_RETENTION_DAYS`.
- [ ] Compute connection limit is at least `DB_MAX_CONNECTIONS × 3 + 2` migration connections.
- [ ] Suspend/scale-to-zero behavior is acceptable for production latency, or production compute is always active.
- [ ] Deletion/export obligations and Neon support escalation path are recorded.
- [ ] `DELETION_LEDGER_RETENTION_DAYS >= POSTGRES_PITR_RETENTION_DAYS + 7`.
- [ ] `scripts/smoke-postgres-gate.sh` passes against both direct URLs.
- [ ] Isolated restore plus `scripts/restore-rehearsal.sh staging` passes before first production deploy.

Never store Neon URLs in source, SST outputs, logs, or client bundles. Set only SST `DatabaseUrlMigrator` and `DatabaseUrlApp` secrets.
