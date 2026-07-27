# ghostpost-evals

Offline-first eval harness for `scan-v1` gold/replay suites.

## Commands

```bash
cargo run --locked --manifest-path evals/Cargo.toml -- manifest list
cargo run --locked --manifest-path evals/Cargo.toml -- manifest verify
cargo run --locked --manifest-path evals/Cargo.toml -- run \
  --suite all --provider replay-fixture --report evals/reports/ci.json
cargo run --locked --manifest-path evals/Cargo.toml -- compare \
  --baseline evals/reports/ci.json --candidate evals/reports/ci.json
```

Exit codes: `0` pass, `10` threshold, `11` hash, `12` privacy, `13` approval, `20` user error.

Providers: `replay-fixture` (default offline), `stub-deterministic`, `deepseek-v4-flash` (approval + API key).

## Layout

See `plans/005-scan-prompt-deepseek-evals.md`. Prompt/schema mirrors must match `backend/prompts` and `backend/src/scan/schema`.

Regenerate gold/replay (synthetic only):

```bash
python3 evals/scripts/gen_gold_replay.py
```
