import { defineConfig, devices } from '@playwright/test';

// E2E runs against the production build served by `wrangler dev`: the same static-assets runtime,
// _headers rules, waitlist Worker, local D1 database, and rate limiter Cloudflare runs at the edge.
const PORT = 8799;

export default defineConfig({
  testDir: './e2e',
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  reporter: [['list'], ['html', { open: 'never', outputFolder: 'e2e/artifacts/report' }]],
  outputDir: 'e2e/artifacts/results',
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    video: 'on',
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } },
    { name: 'mobile', use: { ...devices['iPhone 13'], defaultBrowserType: 'chromium' } },
  ],
  webServer: {
    command: `astro build && bun run db:migrate:local && wrangler dev --port ${PORT} --ip 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}/`,
    reuseExistingServer: false,
    timeout: 120_000,
    env: { PUBLIC_SITE_URL: 'https://getghostpost.com' },
  },
});
