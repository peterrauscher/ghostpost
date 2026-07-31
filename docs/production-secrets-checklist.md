# Production secrets checklist

Run only with `AWS_PROFILE=ghostpost-deployer`; deployment scripts reject root or any other role. Set each value independently for `staging` and `production` with `npx sst secret set NAME "$VALUE" --stage STAGE`. Never use production values in personal stages.

- `DatabaseUrlMigrator`: Neon direct TLS URL for DDL role.
- `DatabaseUrlApp`: Neon direct TLS URL for DML-only role.
- `WorkosApiKey`, `WorkosClientId`, `WorkosWebhookSecret`, `WorkosCookiePassword`.
- `AppSessionKeys`: rotating session-key JSON.
- `DeepseekApiKey`.
- `ScanLlmApprovalManifestJson`: approved, unexpired manifest; validate before setting.
- `ArchiveFingerprintKeys`: rotating archive HMAC-key JSON.
- `ScanBatchHmacKeys`: rotating batch HMAC-key JSON.

Rotation gate: deploy serially; run migration task before API; verify health, WorkOS authorize, and S3 versioning smokes. Session/cookie key rotation must preserve prior decrypt-only key until its maximum lifetime expires.
