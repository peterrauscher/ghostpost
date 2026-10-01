# Ghostpost landing page

Pre-launch marketing site for Ghostpost, live at **https://getghostpost.com**.

It's built from two pieces:

- **Static site.** Astro builds plain HTML/CSS plus about 3 KB (gzipped) of JS for the live phone demo. Cloudflare serves these files straight from its edge.
- **Waitlist Worker.** One small Cloudflare Worker (`worker/index.ts`) handles `POST /api/waitlist` and nothing else. `run_worker_first: ["/api/*"]` means every other request is a plain static asset and never runs Worker code.

Other notes:

- Mascots and platform icons are imported from `../app/assets`, so the app stays the single source for them. Design tokens in `src/styles/global.css` copy `app/src/theme/tokens.ts`.
- The phone demo (`src/components/PhoneDemo.astro` + `src/demo/`) is a live HTML copy of the app's scan, home, and flag-detail screens. It plays on its own while it's on screen, and the visitor takes over on their first tap.
- Page spacing follows orchid.ai: 120px gutters, a 1200px column, 64px between text and media, 176px padding around the features band, and 80px between rows.
- Notched phones: the browser keeps a strip above the page for the status bar and fills it with the canvas background (`--page-canvas`, painted on `html` and `body`). Pages that open with the welcome gradient pass `canvas="var(--welcome-top)"` to `Base.astro` so that strip matches the top of the page instead of showing up as a white band.

## Search, social, and AI discoverability

`src/lib/site.ts` holds the title, description, and share-card details, plus `INDEXABLE_PATHS`. Everything below reads from it:

- **Head tags** (`src/layouts/Base.astro`): canonical URL, `robots` meta (utility pages pass `noindex`), Open Graph, and Twitter `summary_large_image` tags.
- **Social card**: `public/og.png` (1200×630). It's rendered by `bun run og` (`scripts/og-card.ts`) with the site font and mascot; rerun it and commit the PNG when the copy or branding changes.
- **Structured data**: the home page embeds JSON-LD for `Organization`, `WebSite`, and `MobileApplication`.
- **Generated files** (`src/pages/*.ts`, prerendered at build time): `/robots.txt` (points at the sitemap, blocks `/api/`), `/sitemap.xml` (`INDEXABLE_PATHS` only), `/llms.txt` (a Markdown summary for LLM agents, per [llmstxt.org](https://llmstxt.org/)), and `/site.webmanifest`.

When you add a page that should be indexed, add it to `INDEXABLE_PATHS` and to the link list in `src/pages/llms.txt.ts`.

## Waitlist

`src/components/WaitlistForm.astro` appears twice: in the hero and in the closing `#join` section. Every "join the waitlist" link points at `#join`.

The form takes either an email or a phone number:

- **Parsing.** `src/lib/contact.ts` parses the input. The form uses it for instant feedback and the Worker uses it as the real check.
- **Normalization.** Emails are lowercased. Phone numbers are stored in E.164 format. A number without a `+` is read as US/Canada.
- **Storage.** Signups go to the D1 database `ghostpost-waitlist`, table `waitlist` (see `migrations/`). Each contact is unique, and joining again is a silent no-op, so the response never reveals whether someone is already on the list.
- **Spam protection.**
  - A honeypot field (`company`).
  - A limit of 5 submissions per minute per IP (the `WAITLIST_LIMITER` binding). The IP comes from `CF-Connecting-IP`, which Cloudflare sets and clients can't forge.
- **Without JavaScript.** The form still posts, and the visitor is redirected to `/joined/` on success or `/oops/` on an error.

Reading signups:

```bash
bunx wrangler d1 execute ghostpost-waitlist --remote --command "SELECT contact, kind, source, created_at FROM waitlist ORDER BY id DESC"
```

## Environment

| Variable | Used for |
| --- | --- |
| `PUBLIC_SITE_URL` | This site's public origin. Used for the canonical and Open Graph URLs. Required at build time; `astro dev` reads it from `.env.development`. |

Bindings (`wrangler.jsonc`): `DB` (D1), `WAITLIST_LIMITER` (rate limit), and `ASSETS`. After changing bindings, run `bun run types` to regenerate `worker/worker-configuration.d.ts`.

## Run

```bash
bun install
bun run dev        # http://localhost:4321, UI only (the waitlist API needs the Worker, so use preview for it)
PUBLIC_SITE_URL=http://127.0.0.1:8788 bun run preview   # full site + Worker + local D1 via wrangler, http://127.0.0.1:8788
bun run check      # astro check + worker typecheck
```

## Deploy

Deploys happen in CI (`.github/workflows/landing.yml`):

- **Every PR and push** that touches `landing/`, `app/assets/`, or the workflow file runs the checks and the E2E suite.
- **Pushes to `master`** then run `bun run deploy`. That applies any pending D1 migrations, builds the site, and runs `wrangler deploy`. The Worker is attached to the `getghostpost.com` custom domain.

The deploy needs one repository secret, **`CLOUDFLARE_API_TOKEN`**. Create it from the "Edit Cloudflare Workers" template with these permissions:

- Account: Workers Scripts:Edit
- Account: D1:Edit
- Zone (`getghostpost.com`): Workers Routes:Edit
- Zone (`getghostpost.com`): Zone:Read

The account ID is already set in `wrangler.jsonc`.

To deploy manually: `bunx wrangler login && PUBLIC_SITE_URL=https://getghostpost.com bun run deploy`.

Caching (see `public/_headers`):

- `/_astro/*` files have content hashes in their names and are sent with `Cache-Control: public, max-age=31536000, immutable`.
- HTML is sent with `public, max-age=0, must-revalidate` plus an ETag. It's served from the edge but revalidated on each visit, so a new deploy shows up right away.

## Font

`src/fonts/sn-pro-latin.woff2` is a Latin subset of `app/assets/fonts/SNPro-VariableFont_wght.ttf` (49 KB instead of 328 KB), with the variable weight axis kept. To regenerate it:

```bash
uvx --from 'fonttools[woff]' pyftsubset ../app/assets/fonts/SNPro-VariableFont_wght.ttf \
  --unicodes='U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+2000-206F,U+2074,U+20AC,U+2122,U+2190-2199,U+2212,U+2215,U+25CF,U+2713,U+24D8' \
  --flavor=woff2 --layout-features='*' --output-file=src/fonts/sn-pro-latin.woff2
```

## E2E

```bash
bunx playwright install chromium   # first time only
bun run test:e2e
```

The test suite builds the site, migrates the local D1 database, serves everything with `wrangler dev`, and checks it on desktop and on a mobile viewport:

- edge cache headers and the 404 page
- the demo's autoplay and visitor takeover
- the waitlist:
  - validation
  - email and phone signups persisted in normalized form
  - duplicates and the honeypot
  - the rate limit
  - the no-JavaScript fallback
- that there's no horizontal overflow
- discoverability: robots, sitemap, llms.txt, manifest, the social card size, JSON-LD, and `noindex` on utility pages

Artifacts are written to `e2e/artifacts/`:

- screenshots
- `*-waitlist-rows.json`, the rows read back from D1
- `*-robots.txt`, `*-sitemap.xml`, `*-llms.txt`, `*-site.webmanifest`, the discovery files as served
- a video of each test, under `results/`
- an HTML report, under `report/`

CI uploads the same folder as the `landing-e2e` artifact.
