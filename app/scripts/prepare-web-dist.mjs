// Makes the Expo static export servable by Cloudflare Workers assets, whose
// path normalization percent-encodes `[` `]` `+` and redirects forever:
// - `+not-found.html` becomes `404.html` (served by not_found_handling: 404-page)
// - each dynamic `[param].html` moves to `_dynamic/`, where worker/index.ts maps it
// Fails if the export contains a dynamic route the Worker does not know about.
import { existsSync, mkdirSync, readdirSync, renameSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const dist = new URL("../dist/", import.meta.url).pathname;
// Keep in lockstep with DYNAMIC in worker/index.ts.
const DYNAMIC = { "review/[id].html": "_dynamic/review-id.html" };

function walk(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? walk(path) : [relative(dist, path)];
  });
}

if (!existsSync(join(dist, "+not-found.html"))) throw new Error("dist/+not-found.html missing; run expo export first");
renameSync(join(dist, "+not-found.html"), join(dist, "404.html"));
mkdirSync(join(dist, "_dynamic"), { recursive: true });
for (const [from, to] of Object.entries(DYNAMIC)) {
  if (!existsSync(join(dist, from))) throw new Error(`expected dynamic route ${from} in dist`);
  renameSync(join(dist, from), join(dist, to));
}
const unknown = walk(dist).filter((file) => /[[\]+]/.test(file));
if (unknown.length) throw new Error(`unmapped special routes in dist: ${unknown.join(", ")}`);
console.log("prepared dist for Workers assets");
