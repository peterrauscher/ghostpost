import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";

export function createArchiveBucket(ledgerRetentionDays: number) {
  const key = new aws.kms.Key("ArchiveKey", {
    description: "Ghostpost private archive and deletion-ledger key",
    enableKeyRotation: true,
    deletionWindowInDays: 30,
  });
  new aws.kms.Alias("ArchiveKeyAlias", {
    name: pulumi.interpolate`alias/ghostpost-${$app.stage}-archive`,
    targetKeyId: key.keyId,
  });

  const bucket = new sst.aws.Bucket("ArchiveBucket", {
    policy: [
      { effect: "deny", principals: "*", actions: ["s3:PutObject"], paths: ["*"], conditions: [{ test: "StringNotEquals", variable: "s3:x-amz-server-side-encryption", values: ["aws:kms"] }] },
      { effect: "deny", principals: "*", actions: ["s3:PutObject"], paths: ["*"], conditions: [{ test: "StringNotEquals", variable: "s3:x-amz-server-side-encryption-aws-kms-key-id", values: [key.arn] }] },
    ],
    transform: {
      bucket: (args) => { args.forceDestroy = false; },
    },
  });

  new aws.s3.BucketVersioningV2("ArchiveVersioning", {
    bucket: bucket.name,
    versioningConfiguration: { status: "Enabled" },
  });

  new aws.s3.BucketLifecycleConfigurationV2("ArchiveLifecycle", {
    bucket: bucket.name,
    rules: [
      {
        id: "raw-archives-24h",
        status: "Enabled",
        filter: { prefix: "archives/" },
        expiration: { days: 1 },
        noncurrentVersionExpiration: { noncurrentDays: 1 },
        abortIncompleteMultipartUpload: { daysAfterInitiation: 1 },
      },
      {
        id: "deletion-ledger-retention",
        status: "Enabled",
        filter: { prefix: "deletion-ledger/v1/" },
        expiration: { days: ledgerRetentionDays },
        noncurrentVersionExpiration: { noncurrentDays: ledgerRetentionDays },
      },
    ],
  });


  return { bucket, key };
}
