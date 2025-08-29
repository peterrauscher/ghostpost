### GhostPost – Major Design Decisions

#### Primary auth vs social
- Primary auth is owned by us (email OTP or passkeys to be added).
- Social providers are linked identities for data connection (X/Twitter, Facebook; TikTok custom; IG via Facebook Graph).
- Rationale: reliability, verified email, vendor changes, account recovery.

#### Backend stack
- Cloudflare Workers (Hono) for low overhead, edge performance, and TS-first DX.
- Neon Postgres + Drizzle ORM for SQL + type safety.
- Lucia for sessions (secure cookies) + Arctic for OAuth providers.
- TikTok requires a custom OAuth flow; Instagram linking is via Facebook Graph (not standalone auth).

#### Client app choices
- Expo Router with nested stacks/tabs; onboarding separated from main tabs.
- SVG icons imported as React components using `react-native-svg-transformer`.
- Local `userId` persisted with AsyncStorage for anonymous flow; later replaced by real auth.

#### Data & privacy
- Platform exports processed on-device in future iterations; MVP sends only minimal metadata to backend (connected accounts, status).
- RLS-like behavior enforced at API layer by scoping every request to `userId`.

#### Migrations & environments
- Drizzle Kit generates SQL migrations from schema; Node script applies them.
- Wrangler manages Worker envs; Node/CI handles DB migrations (Workers cannot run them).


