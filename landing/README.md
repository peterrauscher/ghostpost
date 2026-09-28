# Ghostpost landing page

Static marketing site for Ghostpost: Astro builds plain HTML/CSS plus about 3 KB (gzipped) of JS for the live phone demo. Cloudflare serves it as an **assets-only Worker**, so there's no server code, no cold starts, and every file comes straight from Cloudflare's edge.

- Mascots and platform icons are imported from `../app/assets`, so the app stays the single source for them. Design tokens in `src/styles/global.css` copy `app/src/theme/tokens.ts`.
- The phone demo (`src/components/PhoneDemo.astro` + `src/demo/`) is a live HTML copy of the app's scan, home, and flag-detail screens. It plays on its own while it's on screen, and the visitor takes over on their first tap.
- Page spacing follows orchid.ai: 120px gutters, a 1200px column, 64px between text and media, 176px padding around the features band, and 80px between rows.

## Environment

| Variable | Used for |
| --- | --- |
| `PUBLIC_APP_URL` | Where "get started" and "log in" point: the deployed Expo web app (`https://$GHOSTPOST_APP_DOMAIN`). |
| `PUBLIC_SITE_URL` | This site's public origin. Used for the canonical and Open Graph URLs. |

Both are required and validated at build time. `astro dev` reads the local defaults from `.env.development`.

## Run

```bash
bun install
bun run dev                     # http://localhost:4321, hot reload

PUBLIC_APP_URL=https://app.example.com PUBLIC_SITE_URL=https://example.com bun run build
bun run preview                 # serves dist/ via wrangler (workerd), same runtime + _headers as production
```

## Deploy (Cloudflare)

```bash
bunx wrangler login
PUBLIC_APP_URL=https://app.example.com PUBLIC_SITE_URL=https://example.com bun run deploy
```

Then attach the custom domain to the `ghostpost-landing` Worker in the Cloudflare dashboard (or add `routes` to `wrangler.jsonc`).

Caching (see `public/_headers`):

- `/_astro/*` files have content hashes in their names and are sent with `Cache-Control: public, max-age=31536000, immutable`.
- HTML is sent with `public, max-age=0, must-revalidate` plus an ETag. It's served from the edge but revalidated on each visit, so a new deploy shows up right away.
- CSS is inlined into the HTML and the font is preloaded, so the first paint needs one request.

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

The test suite builds the site, serves it with `wrangler dev`, and checks it on desktop and on a mobile viewport:

- edge cache headers and the 404 page
- that every CTA points at `PUBLIC_APP_URL`
- that the demo autoplays and a visitor can take it over
- that there's no horizontal overflow

Artifacts are written to `e2e/artifacts/`: `*-hero.png`, `*-demo-home.png`, `*-demo-clean.png`, `*-full.png`, a video of each test under `results/`, and an HTML report under `report/`.
