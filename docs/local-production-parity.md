# Local versus production infrastructure

| Concern | Local | Production |
|---|---|---|
| Postgres | Docker PostgreSQL 17 | Neon direct TLS endpoints, separate DDL/DML roles, PITR |
| Object storage | MinIO, versioning, no KMS | S3 versioning, SSE-KMS, immutable deletion ledger lifecycle |
| Runtime | Docker Compose API/worker | ECS Fargate arm64, public subnets, no NAT |
| Web | Expo dev/export | SST StaticSite custom domain |
| Secrets | ignored `.env` | SST stage secrets |
| Auth | WorkOS test environment | reviewed WorkOS production environment |
| Model | deterministic local stub allowed | DeepSeek only with approved unexpired release manifest |

MinIO cannot prove AWS KMS policy, IAM least privilege, ALB health behavior, CloudWatch alarms, Neon PITR, or WorkOS production redirects. Staging smokes remain mandatory.
