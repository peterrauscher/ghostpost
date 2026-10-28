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

Exit codes: `0` pass, `10` threshold, `11` hash, `12` privacy, `20` user error.

Providers: `replay-fixture` (default offline), `stub-deterministic`, `deepseek-flash` (DeepSeek V4.1 Flash; `DEEPSEEK_API_KEY`), `jev` (TypeSafe Jev via `backend/crates/scan-jev`; `TYPESAFE_API_KEY`). Live providers require only their API key, not an approval manifest. Suite cost uses peak-hour rates for DeepSeek; DeepSeek bills half that off-peak.

`jev` pads each gold case with neighbor gold posts up to `SCAN_JEV_MAX_POSTS` (default 8) so grading sees production batch effects, and honors the `SCAN_JEV_*` thresholds. Validator fixtures (`expect_invariant_failure`) apply only to `replay-fixture`; live providers record them as `REPLAY_ONLY_FIXTURE`. Live report: `evals/reports/jev.json`.

## Layout

Prompt/schema mirrors must match `backend/prompts` and `backend/src/scan/schema`.

Regenerate gold/replay (synthetic only):

```bash
python3 evals/scripts/gen_gold_replay.py
```
