# Deletion replay after database restore

Purpose: prevent a point-in-time restore from resurrecting accounts deleted after restored timestamp.

1. Create an isolated Neon recovery branch at exact RFC3339 restore point. Do not repoint API.
2. Export direct TLS URLs for isolated branch as `RESTORE_DATABASE_URL_MIGRATOR` and `RESTORE_DATABASE_URL_APP`.
3. Confirm archive bucket deletion ledger retention covers restore point and WorkOS credentials target correct environment.
4. Set `RESTORE_POINT`, `ALLOW_RESTORE_CUTOVER=YES`, DeepSeek validation inputs, domains, region, retention values, and git SHA.
5. Run `scripts/restore-rehearsal.sh STAGE`. Script creates one UNLOGGED guard hash, runs one-shot `DeletionReplay`, and stops on malformed/missing markers, missing WorkOS identity, provider failure, exact-version object deletion failure, or remaining user row.
6. Script updates SST database secrets only after replay exits zero, then runs serial migration/API/web deployment.
7. Verify `/health/ready`, WorkOS authorize, S3 versioning, account deletion alarm state, and restored user absence.
8. Preserve Neon pre-cutover branch until review closes. Never delete ledger objects during recovery.

If replay fails: keep restored branch isolated, do not change SST database secrets, inspect redacted JSON logs, repair prerequisite, and rerun same marker set. Provider `not_found` is success; other provider errors are not.
