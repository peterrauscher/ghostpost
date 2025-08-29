### GhostPost Architecture

#### Overview
- **Client**: Expo (React Native) app using Expo Router and TypeScript.
- **API**: Cloudflare Workers with Hono, Drizzle ORM, Neon Postgres.
- **Auth**: Lucia sessions; OAuth via Arctic (Facebook, X/Twitter); TikTok planned; Instagram via Facebook Graph linking.

#### Repository Layout (high-level)
- `app/` – Expo Router screens and layouts (onboarding, tabs, modal, not-found). Exact folder names may evolve (e.g., `onboarding/` or `(onboarding)/`).
- `assets/` – images, icons (SVG brand assets), fonts.
- `components/`, `constants/` – shared UI and theme utilities.
- `lib/` – client utilities (API helpers, local user id persistence).
- `server/` – Worker API
  - `src/` – Hono app and routes
    - `auth/` – Lucia config, OAuth providers, auth routes
    - `db/` – Drizzle schema and client, migrations runner
    - `index.ts` – API entry; mounts `/auth/*` and app routes
  - `drizzle/` – SQL migrations (generated)
  - `wrangler.toml` – Worker config and envs

#### Data Model (Postgres)
- `users`: internal user record (id UUID pk, email nullable, created_at)
- `sessions`: Lucia sessions (id, user_id fk, expires_at, created_at)
- `user_identities`: social identities linked to users
  - `provider` enum: `instagram | tiktok | twitter | facebook`
  - unique on `(provider, provider_user_id)`
- `profiles`: optional profile (1:1 with users)
- `connected_accounts`: UX/status per provider; unique `(user_id, provider)`
- `uploads`: platform export uploads (file_name, byte_size, status)

#### Client ↔ API Contracts (MVP)
- `POST /connected-accounts` → { userId, provider, status } upsert; ensures `users` row exists
- `GET /connected-accounts/:userId` → list of connected provider rows
- Auth endpoints (server only; mobile deep-linking pending):
  - `/auth/login/facebook`, `/auth/callback/facebook`
  - `/auth/login/twitter`, `/auth/callback/twitter`

#### Environments
- Dev DB (Neon branch) via Wrangler `--env dev` secret binding.
- Worker non-secret vars (redirect URIs) in `wrangler.toml` under `[env.<name>.vars]`.


