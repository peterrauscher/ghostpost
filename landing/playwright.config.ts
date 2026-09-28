import { defineConfig, devices } from '@playwright/test';

// E2E runs against the production build served by `wrangler dev`, i.e. the same static-assets
// runtime (and the same _headers rules) Cloudflare uses at the edge.
const PORT = 8799;
export const TEST_APP_URL = 'https://app.ghostpost.test';

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
    command: `astro build && wrangler dev --port ${PORT} --ip 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}/`,
    reuseExistingServer: false,
    timeout: 120_000,
    env: {
      PUBLIC_APP_URL: TEST_APP_URL,
      PUBLIC_SITE_URL: 'https://ghostpost.test',
    },
  },
});
