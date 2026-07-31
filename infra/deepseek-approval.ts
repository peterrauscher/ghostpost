import fs from "node:fs";
import path from "node:path";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

export function validateDeepseekApproval(manifestPath: string | undefined, promptHash: string | undefined, schemaHash: string | undefined) {
  if (!manifestPath || !promptHash || !schemaHash) throw new Error("DeepSeek manifest path and release hashes are required");
  const schema = JSON.parse(fs.readFileSync(path.join(process.cwd(), "infra/deepseek-approval.schema.json"), "utf8"));
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const ajv = new Ajv2020({ allErrors: true });
  addFormats(ajv);
  if (!ajv.validate(schema, manifest)) throw new Error(`invalid DeepSeek approval manifest: ${ajv.errorsText(ajv.errors)}`);
  const approved = manifest as Record<string, string>;
  if (Date.parse(approved.effectiveAt) > Date.now() || Date.parse(approved.expiresAt) <= Date.now()) throw new Error("DeepSeek approval is not currently effective");
  if (approved.promptHash !== promptHash || approved.schemaHash !== schemaHash) throw new Error("DeepSeek approval hashes do not match release tuple");
  return JSON.stringify(approved);
}
