#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const manifestPath = process.argv[2] ?? process.env.SCAN_LLM_APPROVAL_MANIFEST_PATH;
if (!manifestPath) throw new Error("manifest path argument or SCAN_LLM_APPROVAL_MANIFEST_PATH is required");
const promptHash = process.env.SCAN_PROMPT_SHA256;
const schemaHash = process.env.SCAN_SCHEMA_SHA256;
if (!promptHash || !schemaHash) throw new Error("SCAN_PROMPT_SHA256 and SCAN_SCHEMA_SHA256 are required");
const root = path.dirname(fileURLToPath(import.meta.url));
const schema = JSON.parse(fs.readFileSync(path.join(root, "../infra/deepseek-approval.schema.json"), "utf8"));
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const ajv = new Ajv2020({ allErrors: true });
addFormats(ajv);
if (!ajv.validate(schema, manifest)) throw new Error(`invalid DeepSeek approval manifest: ${ajv.errorsText(ajv.errors)}`);
if (Date.parse(manifest.effectiveAt) > Date.now()) throw new Error("approval is not effective yet");
if (Date.parse(manifest.expiresAt) <= Date.now()) throw new Error("approval is expired");
if (manifest.promptHash !== promptHash || manifest.schemaHash !== schemaHash) throw new Error("approval release hashes do not match build tuple");
if (manifest.retentionDecision !== "approved" && manifest.retentionDecision !== "no_retention") throw new Error("retention is not approved");
if (manifest.trainingUseDecision !== "approved" && manifest.trainingUseDecision !== "no_training") throw new Error("training use is not approved");
process.stdout.write("DeepSeek approval manifest valid\n");
