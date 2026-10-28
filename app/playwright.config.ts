import { defineConfig, devices } from '@playwright/test';

const baseURL = process.env.E2E_BASE_URL;
if (!baseURL || !['http:', 'https:'].includes(new URL(baseURL).protocol)) {
  throw new Error('E2E_BASE_URL must name the deployed app origin (for example https://app.getghostpost.com)');
}
if (!process.env.E2E_WORKOS_API_KEY?.trim()) {
  throw new Error('E2E_WORKOS_API_KEY is required for isolated WorkOS test identities; it stays in the Node test process');
}

export default defineConfig({
  testDir: './e2e',
  testMatch: 'auth.spec.ts',
  timeout: 180_000,
  expect: { timeout: 20_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: [
    ['list'],
    ['html', { open: 'never', outputFolder: 'e2e/artifacts/report' }],
    ['json', { outputFile: 'e2e/artifacts/results.json' }],
  ],
  outputDir: 'e2e/artifacts/results',
  use: {
    baseURL: new URL(baseURL).origin,
    actionTimeout: 20_000,
    navigationTimeout: 30_000,
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    video: 'on',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], browserName: 'chromium' } },
    { name: 'mobile-webkit', use: { ...devices['iPhone 13'], browserName: 'webkit' } },
  ],
});
