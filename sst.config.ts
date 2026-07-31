/// <reference path="./.sst/platform/config.d.ts" />
export default $config({
  app(input) {
    return {
      name: "ghostpost",
      home: "aws",
      providers: { aws: { region: process.env.AWS_REGION! } },
      protect: input.stage === "production",
      removal: input.stage === "production" ? "retain" : "remove",
    };
  },
  async run() {
    const aws = await import("@pulumi/aws");
    const { createArchiveBucket } = await import("./infra/archive-bucket");
    const { validateDeepseekApproval } = await import("./infra/deepseek-approval");
    const approvedManifest = validateDeepseekApproval(process.env.SCAN_LLM_APPROVAL_MANIFEST_PATH, process.env.SCAN_PROMPT_SHA256, process.env.SCAN_SCHEMA_SHA256);
    const appDomain = process.env.GHOSTPOST_APP_DOMAIN;
    const apiDomain = process.env.GHOSTPOST_API_DOMAIN;
    const cookieSite = process.env.GHOSTPOST_COOKIE_SITE;
    const appRole = process.env.DATABASE_APP_ROLE;
    const region = process.env.AWS_REGION;
    const gitSha = process.env.GIT_SHA ?? process.env.GITHUB_SHA;
    const pitrDays = Number(process.env.POSTGRES_PITR_RETENTION_DAYS);
    const ledgerDays = Number(process.env.DELETION_LEDGER_RETENTION_DAYS);
    if (!appDomain || !apiDomain || !cookieSite || !region || !gitSha) throw new Error("domains, AWS_REGION, and GIT_SHA are required");
    if (!appRole || !/^[a-z_][a-z0-9_]*$/.test(appRole)) throw new Error("DATABASE_APP_ROLE must be a simple unquoted Postgres identifier");
    const sameSite = (domain: string) => domain === cookieSite || domain.endsWith(`.${cookieSite}`);
    if (!sameSite(appDomain) || !sameSite(apiDomain)) throw new Error("app and API domains must share GHOSTPOST_COOKIE_SITE");
    if (!Number.isInteger(pitrDays) || pitrDays < 1 || !Number.isInteger(ledgerDays) || ledgerDays < pitrDays + 7) throw new Error("deletion-ledger retention must be at least PITR retention plus 7 days");

    const databaseUrlMigrator = new sst.Secret("DatabaseUrlMigrator");
    const databaseUrlApp = new sst.Secret("DatabaseUrlApp");
    const workosApiKey = new sst.Secret("WorkosApiKey");
    const workosClientId = new sst.Secret("WorkosClientId");
    const workosWebhookSecret = new sst.Secret("WorkosWebhookSecret");
    const workosCookiePassword = new sst.Secret("WorkosCookiePassword");
    const appSessionKeys = new sst.Secret("AppSessionKeys");
    const deepseekApiKey = new sst.Secret("DeepseekApiKey");
    const scanApprovalManifestJson = new sst.Secret("ScanLlmApprovalManifestJson");
    const archiveFingerprintKeys = new sst.Secret("ArchiveFingerprintKeys");
    const scanBatchHmacKeys = new sst.Secret("ScanBatchHmacKeys");
    const validatedApprovalSecret = scanApprovalManifestJson.value.apply(value => {
      if (JSON.stringify(JSON.parse(value)) !== approvedManifest) throw new Error("ScanLlmApprovalManifestJson does not match validated approval artifact");
      return value;
    });

    const vpc = new sst.aws.Vpc("Vpc", { az: 2 });
    const cluster = new sst.aws.Cluster("Cluster", { vpc });
    const archive = createArchiveBucket(ledgerDays);
    const opsTopic = new aws.sns.Topic("OpsTopic", { name: `ghostpost-${$app.stage}-ops` });
    const image = { context: "./backend", dockerfile: "Dockerfile", target: "runtime", tags: [gitSha] };
    const logRetention = $app.stage === "production" ? "1 month" as const : "2 weeks" as const;

    const migrations = new sst.aws.Task("Migrations", {
      cluster, public: true, architecture: "arm64", cpu: "0.25 vCPU", memory: "0.5 GB", storage: "20 GB", image,
      command: ["migrate"],
      environment: { DATABASE_URL: databaseUrlMigrator.value, DATABASE_APP_ROLE: appRole, RUST_LOG: "info", LOG_FORMAT: "json", MIGRATION_MAX_CONNECTIONS: "2", MIGRATION_LOCK_TIMEOUT_SECS: "60" },
      logging: { name: `/ghostpost/${$app.stage}/migrations`, retention: logRetention },
    });

    const archiveObjects = archive.bucket.arn.apply(arn => `${arn}/archives/*`);
    const ledgerObjects = archive.bucket.arn.apply(arn => `${arn}/deletion-ledger/v1/*`);
    const archivePermissions = [
      { actions: ["s3:ListBucket", "s3:GetBucketVersioning"], resources: [archive.bucket.arn] },
      { actions: ["s3:GetObject", "s3:GetObjectVersion", "s3:PutObject", "s3:DeleteObjectVersion"], resources: [archiveObjects] },
      { actions: ["s3:GetObject", "s3:GetObjectVersion", "s3:PutObject"], resources: [ledgerObjects] },
      { actions: ["kms:Encrypt", "kms:Decrypt", "kms:GenerateDataKey"], resources: [archive.key.arn] },
    ];

    const replayPermissions = [
      { actions: ["s3:ListBucket"], resources: [archive.bucket.arn] },
      { actions: ["s3:DeleteObjectVersion"], resources: [archiveObjects] },
      { actions: ["s3:GetObject", "s3:GetObjectVersion", "s3:PutObject"], resources: [ledgerObjects] },
      { actions: ["kms:Encrypt", "kms:Decrypt", "kms:GenerateDataKey"], resources: [archive.key.arn] },
    ];

    const commonRuntime = {
      DATABASE_URL_APP: databaseUrlApp.value, RUST_LOG: "info,sqlx=warn", LOG_FORMAT: "json", DB_MAX_CONNECTIONS: "10", SHUTDOWN_DEADLINE_SECS: "30",
      WORKOS_API_KEY: workosApiKey.value, WORKOS_CLIENT_ID: workosClientId.value, WORKOS_WEBHOOK_SECRET: workosWebhookSecret.value, WORKOS_COOKIE_PASSWORD: workosCookiePassword.value,
      APP_SESSION_KEYS: appSessionKeys.value, AUTH_WEB_REDIRECT_URI: `https://${appDomain}/auth/callback`, AUTH_NATIVE_REDIRECT_URI: "ghostpost://auth/callback",
      AUTH_WEB_ORIGINS: `https://${appDomain}`, CORS_ALLOWED_ORIGINS: `https://${appDomain}`, DEEPSEEK_API_KEY: deepseekApiKey.value, SCAN_LLM_PROVIDER: "deepseek-v4-flash",
      SCAN_LLM_APPROVAL_MANIFEST_JSON: validatedApprovalSecret, SCAN_BATCH_HMAC_KEYS: scanBatchHmacKeys.value, ARCHIVE_FINGERPRINT_KEYS: archiveFingerprintKeys.value,
      ARCHIVE_BUCKET: archive.bucket.name, ARCHIVE_S3_REGION: region, ARCHIVE_S3_FORCE_PATH_STYLE: "false", ARCHIVE_S3_REQUIRE_KMS: "true", ARCHIVE_S3_KMS_KEY_ID: archive.key.arn,
      ARCHIVE_RAW_RETENTION_HOURS: "24", WORKER_LEASE_SECS: "60", WORKER_HEARTBEAT_SECS: "20", WORKER_SWEEPER_SECS: "30", RESTORE_REPLAY_PENDING: process.env.RESTORE_REPLAY_PENDING ?? "false",
    };

    const api = new sst.aws.Service("Api", {
      cluster, architecture: "arm64", cpu: "0.5 vCPU", memory: "1 GB", storage: "20 GB", image, command: ["serve", "--role", "all"], permissions: archivePermissions,
      environment: { ...commonRuntime, BIND_ADDR: "0.0.0.0:8080" },
      loadBalancer: { public: true, domain: apiDomain, rules: [{ listen: "80/http", redirect: "443/https" }, { listen: "443/https", forward: "8080/http" }], health: { "8080/http": { path: "/health/ready", successCodes: "200", interval: "15 seconds", timeout: "5 seconds", healthyThreshold: 2, unhealthyThreshold: 2 } } },
      health: { command: ["CMD-SHELL", "curl -fsS http://127.0.0.1:8080/health/live || exit 1"], interval: "30 seconds", timeout: "5 seconds", retries: 3, startPeriod: "60 seconds" },
      scaling: { min: 1, max: 3, cpuUtilization: 65, memoryUtilization: 75, scaleOutCooldown: "60 seconds", scaleInCooldown: "5 minutes" },
      capacity: $app.stage === "production" ? undefined : "spot", logging: { name: `/ghostpost/${$app.stage}/api`, retention: logRetention }, wait: true,
      transform: { service: args => { args.deploymentCircuitBreaker = { enable: true, rollback: true }; args.healthCheckGracePeriodSeconds = 60; } },
    });

    const deletionReplay = new sst.aws.Task("DeletionReplay", {
      cluster, public: true, architecture: "arm64", cpu: "0.25 vCPU", memory: "0.5 GB", storage: "20 GB", image,
      command: ["deletion", "replay"], permissions: replayPermissions,
      environment: { RESTORE_DATABASE_URL: databaseUrlMigrator.value, WORKOS_API_KEY: workosApiKey.value, ARCHIVE_BUCKET: archive.bucket.name, ARCHIVE_S3_REGION: region, ARCHIVE_S3_REQUIRE_KMS: "true", ARCHIVE_S3_KMS_KEY_ID: archive.key.arn, RESTORE_REPLAY_PENDING: "true", RUST_LOG: "info", LOG_FORMAT: "json" },
      logging: { name: `/ghostpost/${$app.stage}/deletion-replay`, retention: logRetention },
    });

    const web = new sst.aws.StaticSite("Web", {
      path: "./app", build: { command: "npm ci && npm run export:web", output: "dist" }, environment: { EXPO_PUBLIC_API_URL: api.url }, domain: appDomain,
      dev: { command: "npm start", directory: "./app", url: "http://localhost:8081", autostart: false },
    });

    const alarmDefaults = { actionsEnabled: true, alarmActions: [opsTopic.arn], comparisonOperator: "GreaterThanOrEqualToThreshold" as const, evaluationPeriods: 2, period: 300, statistic: "Sum" as const, namespace: "AWS/ECS" };
    const loadBalancerDimension = api.nodes.loadBalancer.arnSuffix;
    new aws.cloudwatch.MetricAlarm("ApiTargetUnhealthy", { ...alarmDefaults, name: `ghostpost-${$app.stage}-api-target-unhealthy`, namespace: "AWS/ApplicationELB", metricName: "UnHealthyHostCount", threshold: 1, period: 60, statistic: "Maximum", dimensions: { LoadBalancer: loadBalancerDimension } });
    new aws.cloudwatch.MetricAlarm("Api5xxRate", { ...alarmDefaults, name: `ghostpost-${$app.stage}-api-5xx`, namespace: "AWS/ApplicationELB", metricName: "HTTPCode_Target_5XX_Count", threshold: 10, comparisonOperator: "GreaterThanThreshold", evaluationPeriods: 1, dimensions: { LoadBalancer: loadBalancerDimension } });
    new aws.cloudwatch.MetricAlarm("ApiCpuHigh", { ...alarmDefaults, name: `ghostpost-${$app.stage}-api-cpu-high`, metricName: "CPUUtilization", threshold: 85, unit: "Percent", statistic: "Average", dimensions: { ClusterName: cluster.nodes.cluster.name, ServiceName: api.nodes.service.name } });
    new aws.cloudwatch.LogMetricFilter("MigrationFailureMetric", { name: `ghostpost-${$app.stage}-migration-failure`, logGroupName: `/ghostpost/${$app.stage}/migrations`, pattern: "{ $.level = ERROR }", metricTransformation: { name: "MigrationFailure", namespace: "Ghostpost", value: "1" } }, { dependsOn: [migrations] });
    new aws.cloudwatch.MetricAlarm("MigrationFailure", { ...alarmDefaults, name: `ghostpost-${$app.stage}-migration-failure`, namespace: "Ghostpost", metricName: "MigrationFailure", threshold: 1 });
    new aws.cloudwatch.LogMetricFilter("AccountPurgeStalledMetric", { name: `ghostpost-${$app.stage}-account-purge-stalled`, logGroupName: `/ghostpost/${$app.stage}/api`, pattern: "{ $.event = account_purge_stalled }", metricTransformation: { name: "AccountPurgeStalled", namespace: "Ghostpost", value: "1" } }, { dependsOn: [api] });
    new aws.cloudwatch.MetricAlarm("AccountPurgeStalled", { ...alarmDefaults, name: `ghostpost-${$app.stage}-account-purge-stalled`, namespace: "Ghostpost", metricName: "AccountPurgeStalled", threshold: 1 });
    new aws.cloudwatch.LogMetricFilter("DeletionReplayFailureMetric", { name: `ghostpost-${$app.stage}-deletion-replay-failure`, logGroupName: `/ghostpost/${$app.stage}/deletion-replay`, pattern: "{ $.event = deletion_replay_failed }", metricTransformation: { name: "DeletionReplayFailure", namespace: "Ghostpost", value: "1" } }, { dependsOn: [deletionReplay] });
    new aws.cloudwatch.MetricAlarm("DeletionReplayFailure", { ...alarmDefaults, name: `ghostpost-${$app.stage}-deletion-replay-failure`, namespace: "Ghostpost", metricName: "DeletionReplayFailure", threshold: 1 });

    return { apiUrl: api.url, webUrl: web.url, migrationTaskDefinition: migrations.taskDefinition, deletionReplayTaskDefinition: deletionReplay.taskDefinition, archiveBucketName: archive.bucket.name, opsTopicArn: opsTopic.arn };
  },
});
