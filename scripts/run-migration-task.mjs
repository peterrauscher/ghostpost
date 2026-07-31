#!/usr/bin/env node
import { Resource } from "sst";
import { task } from "sst/aws/task";

const run = await task.run(Resource.Migrations);
for (;;) {
  const state = await task.describe(Resource.Migrations, run.arn);
  if (state.status === "STOPPED") {
    const code = state.response.tasks?.[0]?.containers?.[0]?.exitCode;
    if (code !== 0) process.exit(code ?? 1);
    break;
  }
  await new Promise(resolve => setTimeout(resolve, 5000));
}
