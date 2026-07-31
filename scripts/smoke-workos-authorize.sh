#!/bin/sh
set -eu
base=${1:?usage: smoke-workos-authorize.sh https://api.example}
body=$(curl --fail --silent --show-error --max-time 15 "$base/v1/auth/authorize?client=web")
printf '%s' "$body" | node -e '
let value=""; process.stdin.on("data", c => value += c).on("end", () => {
  const url = new URL(JSON.parse(value).authorizationUrl);
  const trusted = url.hostname === "api.workos.com" || url.hostname.endsWith(".authkit.app");
  if (url.protocol !== "https:" || !trusted) process.exit(1);
});'
printf 'WorkOS authorize smoke passed\n'
