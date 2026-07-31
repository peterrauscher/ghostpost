#!/bin/sh
set -eu
: "${ARCHIVE_BUCKET:?required}"
key="smoke/versioning-$(date +%s)-$$"
first=$(printf first | aws s3api put-object --bucket "$ARCHIVE_BUCKET" --key "$key" --server-side-encryption aws:kms --ssekms-key-id "${ARCHIVE_S3_KMS_KEY_ID:?required}" --body /dev/stdin --query VersionId --output text)
second=$(printf second | aws s3api put-object --bucket "$ARCHIVE_BUCKET" --key "$key" --server-side-encryption aws:kms --ssekms-key-id "$ARCHIVE_S3_KMS_KEY_ID" --body /dev/stdin --query VersionId --output text)
[ -n "$first" ] && [ "$first" != None ] && [ -n "$second" ] && [ "$second" != None ] && [ "$first" != "$second" ]
aws s3api delete-object --bucket "$ARCHIVE_BUCKET" --key "$key" --version-id "$first" >/dev/null
aws s3api delete-object --bucket "$ARCHIVE_BUCKET" --key "$key" --version-id "$second" >/dev/null
printf 'S3 versioning smoke passed\n'
