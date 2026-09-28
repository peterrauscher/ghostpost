import { expect, test, type Page } from '@playwright/test';
import { TEST_APP_URL } from '../playwright.config';

/*
 * Ways the landing page can fail that these tests catch:
 * - Edge caching: hashed assets not immutable, or unknown paths not returning the 404 page.
 * - Runtime: any console error / uncaught exception (demo script crash leaves a frozen phone).
 * - Demo autoplay stalls: never leaves the scan screen, never opens a flag, never resolves one.
 * - Takeover: a visitor tap mid-scan leaves the phone stuck, or actions don't update count/risk.
 * - CTAs pointing anywhere other than the configured app URL.
 * - Mobile: horizontal overflow or the phone not fitting the viewport.
 * Artifacts: full-page screenshots per project + a video of every test under e2e/artifacts/.
 */

/** Viewport capture centred on the phone (element captures taller than the viewport render blank in Chromium). */
async function capturePhone(page: Page, name: string) {
  await page.locator('.d-screen').evaluate((el) => el.scrollIntoView({ block: 'center', behavior: 'instant' }));
  await page.waitForTimeout(500);
  await page.screenshot({ path: artifact(name) });
}

const artifact = (name: string) => `e2e/artifacts/${test.info().project.name}-${name}.png`;

/** Scroll the whole page once so every scroll-triggered reveal has fired before a full-page capture. */
async function revealAll(page: Page) {
  const height = await page.evaluate(() => document.documentElement.scrollHeight);
  for (let y = 0; y < height; y += 400) {
    await page.evaluate((top) => window.scrollTo({ top, behavior: 'instant' }), y);
    await page.waitForTimeout(60);
  }
  await page.waitForTimeout(1600);
}

function collectErrors(page: Page) {
  const errors: string[] = [];
  page.on('console', (message) => message.type() === 'error' && errors.push(message.text()));
  page.on('pageerror', (error) => errors.push(error.message));
  return errors;
}

test('edge headers: hashed assets are immutable, HTML revalidates, unknown paths 404', async ({ request }) => {
  const home = await request.get('/');
  expect(home.status()).toBe(200);
  expect(home.headers()['content-type']).toContain('text/html');
  expect(home.headers()['x-content-type-options']).toBe('nosniff');
  // HTML is served from the edge but revalidated by ETag, so a deploy is visible immediately.
  expect(home.headers()['cache-control']).toBe('public, max-age=0, must-revalidate');
  expect(home.headers()['etag']).toBeTruthy();

  const asset = (await home.text()).match(/\/_astro\/[\w.-]+\.(?:js|woff2|webp)/)?.[0];
  expect(asset, 'page references a hashed /_astro asset').toBeTruthy();
  const assetResponse = await request.get(asset!);
  expect(assetResponse.status()).toBe(200);
  expect(assetResponse.headers()['cache-control']).toBe('public, max-age=31536000, immutable');

  const missing = await request.get('/definitely-not-a-page');
  expect(missing.status()).toBe(404);
  expect(await missing.text()).toContain('this page ghosted you.');
});

test('page renders, CTAs target the app, demo autoplays scan → review → clean up', async ({ page }) => {
  const errors = collectErrors(page);
  await page.goto('/');

  await expect(page.getByRole('heading', { level: 1 })).toHaveText(/let’s clean\s*your slate\./);
  await page.screenshot({ path: artifact('hero') });
  const ctaHrefs = await page.locator('a.pill', { hasText: 'get started' }).evaluateAll((links) =>
    links.map((link) => (link as HTMLAnchorElement).href),
  );
  expect(ctaHrefs.length).toBeGreaterThan(3);
  for (const href of ctaHrefs) expect(href).toBe(`${TEST_APP_URL}/`);
  await expect(page.getByRole('link', { name: 'log in' }).first()).toHaveAttribute('href', `${TEST_APP_URL}/login`);

  const demo = page.locator('[data-demo]');
  const screen = demo.locator('.d-screen');
  await demo.scrollIntoViewIfNeeded();

  await expect(screen).toHaveAttribute('data-view', 'scan');
  await expect(screen).toHaveAttribute('data-view', 'home', { timeout: 10_000 });
  await expect(demo.locator('[data-field="count"]')).toHaveText('4');
  await capturePhone(page, 'demo-home');
  await expect(screen).toHaveAttribute('data-view', 'detail', { timeout: 10_000 });
  await expect(demo.locator('[data-field="d-quote"]')).toContainText('never drinking again');
  // Autoplay taps "Delete Ghostpost copy": high-risk flag disappears, overall risk drops to Medium.
  await expect(demo.locator('[data-field="count"]')).toHaveText('3', { timeout: 10_000 });
  await expect(demo.locator('[data-field="risk-label"]')).toHaveText('Medium');
  await expect(demo.locator('.d-row')).toHaveCount(3);

  await revealAll(page);
  await page.screenshot({ path: artifact('full'), fullPage: true });
  expect(errors).toEqual([]);
});

test('visitor can take over mid-scan and drive the demo', async ({ page }) => {
  const errors = collectErrors(page);
  await page.goto('/');
  const demo = page.locator('[data-demo]');
  const screen = demo.locator('.d-screen');
  await demo.scrollIntoViewIfNeeded();
  await expect(screen).toHaveAttribute('data-view', 'scan');

  await screen.click({ position: { x: 40, y: 400 } });
  await expect(demo).toHaveAttribute('data-mode', 'manual');
  await expect(demo.getByText('you’re driving 👻')).toBeVisible();
  // The interrupted scan still finishes on its own.
  await expect(screen).toHaveAttribute('data-view', 'home', { timeout: 10_000 });

  await demo.locator('.d-row__btn[data-id="p3"]').click();
  await expect(screen).toHaveAttribute('data-view', 'detail');
  await expect(demo.locator('[data-field="d-platform"]')).toHaveText('TikTok');
  await demo.locator('[data-action="back"]').click();
  await expect(screen).toHaveAttribute('data-view', 'home');
  await expect(demo.locator('[data-field="count"]')).toHaveText('4');

  // Resolve every flag through the banner CTA; each action shrinks the list.
  for (const [action, remaining] of [['delete', 3], ['archive', 2], ['keep', 1], ['resolve', 0]] as const) {
    await demo.locator('[data-action="review"]').click();
    await expect(screen).toHaveAttribute('data-view', 'detail');
    await demo.locator(`[data-action="${action}"]`).click();
    await expect(screen).toHaveAttribute('data-view', 'home');
    await expect(demo.locator('[data-field="count"]')).toHaveText(String(remaining));
  }
  await expect(demo.locator('[data-field="headline"]')).toHaveText('your slate is clean. nice work ✨');
  await expect(demo.locator('[data-field="risk-label"]')).toHaveText('Clear');
  await expect(demo.getByText('nothing left to review 👻')).toBeVisible();
  await capturePhone(page, 'demo-clean');

  // "scan again" restarts with a fresh set of flags.
  await demo.locator('[data-action="review"]').click();
  await expect(screen).toHaveAttribute('data-view', 'scan');
  await expect(screen).toHaveAttribute('data-view', 'home', { timeout: 10_000 });
  await expect(demo.locator('[data-field="count"]')).toHaveText('4');

  await demo.locator('[data-replay]').click();
  await expect(demo).toHaveAttribute('data-mode', 'auto');
  expect(errors).toEqual([]);
});

test('layout fits the viewport without horizontal scroll', async ({ page }) => {
  await page.goto('/');
  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth,
  }));
  expect(scrollWidth).toBeLessThanOrEqual(innerWidth);
  const phone = await page.locator('.d-phone').boundingBox();
  expect(phone!.x).toBeGreaterThanOrEqual(0);
  expect(phone!.x + phone!.width).toBeLessThanOrEqual(innerWidth);
});
