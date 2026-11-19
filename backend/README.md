# Ghostpost backend

Single-package Rust/Axum kernel: config, health, SQLx migrations, tenant-safe
schema, and a fenced Postgres work queue.

## Prerequisites

- Rust stable ≥ 1.80
- Docker Engine (Postgres 17)
- `sqlx-cli` for offline data: `cargo install sqlx-cli --no-default-features --features rustls,postgres`

## Local Postgres

```bash
docker compose -f docker-compose.postgres.yml up -d --wait
cp .env.example .env
cargo run --locked -- migrate
```

## Serve roles

```bash
cargo run --locked -- serve --role all    # API + worker (local default)
cargo run --locked -- serve --role api
cargo run --locked -- serve --role worker
```

Health:

- `GET /health/live` — process up
- `GET /health/ready` — DML pool + `SELECT 1` (503 Problem+JSON when not ready)

## Tests

```bash
cargo test --locked
cargo test --locked --test postgres_integration -- --ignored --test-threads=1
```

## Offline SQLx

```bash
cargo sqlx prepare -- --all-targets
```

CI builds with `SQLX_OFFLINE=true`.

## Instagram and TikTok archives

Both platforms use the same ZIP indexing, integrity checks, resource limits,
bounded parse channel, staging, and fenced commit path as Reddit and X.
The normalized schema remains version 1. Direct messages, engagement history,
profile data, and media binaries are not imported.

- Instagram: JSON posts (numbered shards), carousel captions, stories, reels,
  archived posts, and post/reel comments. Current Accounts Center and older
  `content/` and `comments/` layouts are recognized, including an export root
  folder. Latin-1-escaped UTF-8 is repaired when unambiguous. Textless media is
  skipped. HTML is intentionally unsupported: version-dependent markup does
  not provide a reliable caption boundary without an HTML parser. Request JSON;
  an HTML-only ZIP fails with `no_supported_content`.
- TikTok: streaming `user_data_tiktok.json` or `user_data.json`, including known
  Comment, Post, Video, Activity, and Your Activity wrappers and key variants.
  TXT Comments, Videos, and Posts blocks support multiline text. Dates without
  timezone are interpreted as UTC. Captionless posts are skipped. Explicit
  nonpublic visibility is excluded; legacy records without visibility metadata
  cannot be checked for audience. TXT has no escaping, so recognized field
  labels at the start of a content line are interpreted as metadata.

New fixture trees under `tests/fixtures/instagram` and `tests/fixtures/tiktok`
are entirely synthetic. Tests generate ZIPs at runtime. Provenance manifests
mark them `synthetic_tree`, not approved real exports. Tree SHA-256 hashes cover
sorted relative UTF-8 paths and file contents, each framed by a big-endian
64-bit byte length. Existing Reddit/X real-export provenance gates remain.
New parsers are marked compatible, not confirmed against reviewed real exports.
Fallback identities depend on the export's source layout and record order;
re-sharding or reordering an export can produce different identities.

Layout references:
- [Meta Accounts Center export workflow](https://about.fb.com/news/2023/10/manage-your-information-across-apps/)
- [TikTok download workflow](https://support.tiktok.com/en/account-and-privacy/personalized-ads-and-data/requesting-your-data)
- [Observed Instagram export schema](https://github.com/anand-loop/picnic/blob/main/docs/instagram-export-format.md)
- [Instagram layout comparison](https://github.com/ethanriverpage/memoria/blob/main/docs/Instagram-Export-Format-Comparison.md)
- [Official TikTok fields](https://developers.tiktok.com/docs/en/data-portability-data-types)
- [TikTok export parser and wrapper variants](https://github.com/abrignoni/RLEAPP/blob/main/scripts/artifacts/tikTokArchiveJson.py)
- [TikTok TXT parser evidence](https://github.com/kenn-io/msgvault/blob/main/internal/tiktokexport/text.go)
