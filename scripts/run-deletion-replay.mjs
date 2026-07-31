#!/usr/bin/env node
import { Resource } from "sst";
import { task } from "sst/aws/task";

const restorePoint = process.env.RESTORE_POINT;
const guardToken = process.env.RESTORE_GUARD_TOKEN;
const databaseUrl = process.env.RESTORE_DATABASE_URL;
if (!restorePoint || !guardToken || !databaseUrl) throw new Error("RESTORE_POINT, RESTORE_GUARD_TOKEN, and RESTORE_DATABASE_URL are required");
const run = await task.run(Resource.DeletionReplay, { RESTORE_POINT: restorePoint, RESTORE_GUARD_TOKEN: guardToken, RESTORE_DATABASE_URL: databaseUrl });
for (;;) {
  const state = await task.describe(Resource.DeletionReplay, run.arn);
  if (state.status === "STOPPED") {
    const code = state.response.tasks?.[0]?.containers?.[0]?.exitCode;
    if (code !== 0) process.exit(code ?? 1);
    break;
  }
  await new Promise(resolve => setTimeout(resolve, 5000));
}
