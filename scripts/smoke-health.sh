#!/bin/sh
set -eu
base=${1:?usage: smoke-health.sh https://api.example}
curl --fail --silent --show-error --max-time 15 "$base/health/live" >/dev/null
curl --fail --silent --show-error --max-time 15 "$base/health/ready" >/dev/null
printf 'health smoke passed\n'
