import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';

/*
 * Ways the landing page can fail that these tests catch:
 * - Edge caching: hashed assets not immutable, or unknown paths not returning the 404 page.
 * - Runtime: any console error / uncaught exception (demo script crash leaves a frozen phone).
 * - Demo autoplay stalls: never leaves the scan screen, never opens a flag, never resolves one.
 * - Takeover: a visitor tap mid-scan leaves the phone stuck, or actions don't update count/risk.
 * - Waitlist: CTAs not leading to the form, bad input accepted, good input not persisted/normalized,
 *   duplicates erroring or leaking membership, no-JS posts dead-ending, spam not rate limited.
 * - Mobile: horizontal overflow or the phone not fitting the viewport.
 * Artifacts: screenshots + a video of every test, and the persisted waitlist rows, under e2e/artifacts/.
 */

/**
 * Each test gets its own client IP so the per-IP rate limiter never couples tests. wrangler dev passes this
 * header through; in production Cloudflare's edge overwrites it with the real address.
 */
const runOctet = Math.floor(Math.random() * 250);
let ipCounter = 0;
const nextIp = () => `10.${runOctet}.${Math.floor(Math.random() * 250)}.${++ipCounter}`;
// Unique per run so persisted rows from earlier runs never satisfy (or collide with) this run's checks.
const runId = Date.now().toString(36);

function queryWaitlist(sql: string): Record<string, unknown>[] {
  const out = execFileSync('bunx', ['wrangler', 'd1', 'execute', 'ghostpost-waitlist', '--local', '--json', '--command', sql], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  return (JSON.parse(out) as { results: Record<string, unknown>[] }[])[0].results;
}

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

test('page renders, CTAs lead to the waitlist, demo autoplays scan → review → clean up', async ({ page }) => {
  const errors = collectErrors(page);
  await page.goto('/');

  await expect(page.getByRole('heading', { level: 1 })).toHaveText(/let’s clean\s*your slate\./);
  await page.screenshot({ path: artifact('hero') });
  const ctas = page.locator('a', { hasText: 'join the waitlist' });
  expect(await ctas.count()).toBeGreaterThan(5);
  for (const href of await ctas.evaluateAll((links) => links.map((link) => link.getAttribute('href')))) {
    expect(href).toBe('#join');
  }
  await expect(page.locator('#join form[data-waitlist]')).toHaveCount(1);
  await expect(page.locator('a[href*="login"]')).toHaveCount(0);

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

test('waitlist: hero takes an email, CTA takes a phone, both persist normalized', async ({ page }) => {
  const errors = collectErrors(page);
  await page.setExtraHTTPHeaders({ 'CF-Connecting-IP': nextIp() });
  const project = test.info().project.name;
  const email = `Jordan+${runId}-${project}@Example.com`;
  const digits = String((Date.now() + project.length) % 10_000_000).padStart(7, '0');
  const phone = `(415) ${digits.slice(0, 3)}-${digits.slice(3)}`;
  await page.goto('/');

  const hero = page.locator('form[data-waitlist]').first();
  const heroStatus = hero.locator('[data-waitlist-status]');
  await hero.getByRole('textbox', { name: 'Email or phone number' }).fill('not a contact');
  await hero.getByRole('button', { name: 'join the waitlist' }).click();
  await expect(heroStatus).toHaveText('that doesn’t look like an email or phone number.');
  await expect(hero).toHaveAttribute('data-state', 'error');

  await hero.getByRole('textbox', { name: 'Email or phone number' }).fill(email);
  await hero.getByRole('button', { name: 'join the waitlist' }).click();
  await expect(heroStatus).toHaveText('you’re on the list 👻 we’ll email you.');
  await page.screenshot({ path: artifact('waitlist-hero-joined') });

  // The header CTA jumps to the closing form.
  await page.locator('header').getByRole('link', { name: 'join the waitlist' }).click();
  await expect(page).toHaveURL(/#join$/);
  const cta = page.locator('#join form[data-waitlist]');
  await expect(cta).toBeInViewport();
  await cta.getByRole('textbox', { name: 'Email or phone number' }).fill(phone);
  await cta.getByRole('button', { name: 'join the waitlist' }).click();
  await expect(cta.locator('[data-waitlist-status]')).toHaveText('you’re on the list 👻 we’ll text you.');
  await cta.screenshot({ path: artifact('waitlist-cta-joined') });

  const rows = queryWaitlist(
    `SELECT contact, kind, source FROM waitlist WHERE contact IN ('${email.toLowerCase()}', '+1415${digits}') ORDER BY kind`,
  );
  expect(rows).toEqual([
    { contact: email.toLowerCase(), kind: 'email', source: 'hero' },
    { contact: `+1415${digits}`, kind: 'phone', source: 'cta' },
  ]);
  writeFileSync(`e2e/artifacts/${project}-waitlist-rows.json`, `${JSON.stringify(rows, null, 2)}\n`);
  expect(errors).toEqual([]);
});

test('waitlist API: duplicates are silent no-ops, honeypot is dropped, bursts are rate limited', async ({ request }) => {
  const headers = { Accept: 'application/json', 'CF-Connecting-IP': nextIp() };
  const contact = `dupe-${runId}-${test.info().project.name}@example.com`;
  const join = (form: Record<string, string>) => request.post('/api/waitlist', { headers, multipart: form });

  const first = await join({ contact, source: 'hero' });
  const again = await join({ contact: contact.toUpperCase(), source: 'cta' });
  expect([first.status(), again.status()]).toEqual([200, 200]);
  expect(await again.json()).toEqual({ ok: true });
  expect(queryWaitlist(`SELECT source FROM waitlist WHERE contact = '${contact}'`)).toEqual([{ source: 'hero' }]);

  const bot = `bot-${runId}-${test.info().project.name}@example.com`;
  expect((await join({ contact: bot, company: 'Acme' })).status()).toBe(200);
  expect(queryWaitlist(`SELECT id FROM waitlist WHERE contact = '${bot}'`)).toEqual([]);

  // Limit is 5 per minute per IP and three were spent above.
  const statuses: number[] = [];
  for (let i = 0; i < 4; i++) statuses.push((await join({ contact: `burst-${i}-${runId}@example.com` })).status());
  expect(statuses).toEqual([200, 200, 429, 429]);
  const limited = await join({ contact: `burst-x-${runId}@example.com` });
  expect(await limited.json()).toEqual({ ok: false, error: 'too many tries. give it a minute and try again.' });

  expect((await request.get('/api/waitlist')).status()).toBe(405);
});

test.describe('without JavaScript', () => {
  test.use({ javaScriptEnabled: false });

  test('waitlist form still works and lands on a confirmation page', async ({ page }) => {
    await page.setExtraHTTPHeaders({ 'CF-Connecting-IP': nextIp() });
    await page.goto('/');
    const hero = page.locator('form[data-waitlist]').first();
    await hero.getByRole('textbox', { name: 'Email or phone number' }).fill(`nojs-${runId}-${test.info().project.name}@example.com`);
    await hero.getByRole('button', { name: 'join the waitlist' }).click();
    await expect(page).toHaveURL(/\/joined\/$/);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('you’re on the list 👻');

    await page.goto('/');
    await page.locator('form[data-waitlist]').first().getByRole('textbox', { name: 'Email or phone number' }).fill('nope');
    await page.locator('form[data-waitlist]').first().getByRole('button', { name: 'join the waitlist' }).click();
    await expect(page).toHaveURL(/\/oops\/$/);
    await expect(page.getByRole('link', { name: 'try again' })).toHaveAttribute('href', '/#join');
  });
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
