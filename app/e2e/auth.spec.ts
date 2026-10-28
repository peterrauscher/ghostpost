import { randomUUID } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import { expect, test as base, type BrowserContext, type Page, type TestInfo } from '@playwright/test';

/* Consumer failures protected at the real browser/provider/backend boundary:
 * - Sign-up disabled, wrong redirects, missing exchange cookies, failed callback,
 *   or a session that disappears after reloading.
 * - Sign-in creates another account or loses an existing user's onboarding.
 * - Logout clears the UI/cookie but leaves the server session usable.
 * - Delete only hides the account: another session survives or WorkOS is never purged.
 * No intercepted responses, pre-issued app sessions, or test-only app endpoints.
 * WorkOS management calls stay in Node, outside browser traces. Only uniquely
 * generated @example.com identities can be queried for codes or cleaned up.
 */

const appOrigin = new URL(process.env.E2E_BASE_URL!).origin;
const workosKey = process.env.E2E_WORKOS_API_KEY!;
const identityPattern = /^ghostpost-e2e-[0-9a-f-]+@example\.com$/;

type Profile = { id: string; email: string };
type WorkosUser = { id: string; email: string };
type Account = {
  email: string;
  startedAt: string;
  apiOrigin?: string;
  providerId?: string;
  profile?: Profile;
  contexts: BrowserContext[];
  evidence: Record<string, unknown>;
};

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Expected an API object');
  return value as Record<string, unknown>;
}

function string(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Expected an API string');
  return value;
}

function list(value: unknown): Record<string, unknown>[] {
  const data = object(value).data;
  if (!Array.isArray(data)) throw new Error('Expected an API data list');
  return data.map(object);
}

function profileFrom(value: unknown): Profile {
  const user = object(object(value).user);
  return { id: string(user.id), email: string(user.email) };
}

async function workos(path: string, method = 'GET'): Promise<{ status: number; body: unknown }> {
  const response = await fetch(`https://api.workos.com${path}`, {
    method,
    headers: { Authorization: `Bearer ${workosKey}` },
    signal: AbortSignal.timeout(20_000),
  });
  const text = await response.text();
  if (!response.ok && response.status !== 404) {
    throw new Error(`WorkOS ${method} ${path.split('?')[0]} returned ${response.status}`);
  }
  return { status: response.status, body: text ? JSON.parse(text) : null };
}

function assertSynthetic(account: Account) {
  if (!identityPattern.test(account.email)) throw new Error('Refusing to operate on a non-test identity');
}

async function findProviderUser(account: Account): Promise<WorkosUser | undefined> {
  assertSynthetic(account);
  const { body } = await workos(`/user_management/users?email=${encodeURIComponent(account.email)}`);
  const users = list(body).map((user) => ({ id: string(user.id), email: string(user.email) }))
    .filter((user) => user.email === account.email);
  if (users.length > 1) throw new Error('Synthetic email unexpectedly matched multiple WorkOS users');
  return users[0];
}

async function verificationCode(account: Account, since: string): Promise<string> {
  assertSynthetic(account);
  let code: string | undefined;
  await expect.poll(async () => {
    const query = new URLSearchParams({ events: 'magic_auth.created', range_start: since, limit: '100' });
    const { body } = await workos(`/events?${query}`);
    const events = list(body).map((event) => ({ createdAt: string(event.created_at), data: object(event.data) }))
      .filter((event) => event.data.email === account.email)
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt));
    if (!events[0]) return false;
    const result = object((await workos(`/user_management/magic_auth/${string(events[0].data.id)}`)).body);
    if (result.email !== account.email) throw new Error('Code belongs to a different identity');
    code = string(result.code);
    return typeof code === 'string' && /^\d{6}$/.test(code);
  }, { message: 'WorkOS issues a code for the isolated test account', timeout: 45_000, intervals: [500, 1000, 2000] }).toBe(true);
  return code!;
}

async function browserMe(page: Page, account: Account): Promise<{ status: number; user?: Profile }> {
  const result = await page.evaluate(async (origin) => {
    const response = await fetch(`${origin}/v1/me`, { credentials: 'include' });
    const body: unknown = response.ok ? await response.json() : null;
    return { status: response.status, body };
  }, account.apiOrigin!);
  return { status: result.status, user: result.status === 200 ? profileFrom(result.body) : undefined };
}

async function authenticate(page: Page, account: Account, kind: 'sign-up' | 'sign-in') {
  assertSynthetic(account);
  await page.goto(kind === 'sign-up' ? '/' : '/login');
  const authorization = page.waitForResponse((response) =>
    new URL(response.url()).pathname === '/v1/auth/authorize' && response.request().method() === 'GET');
  await page.getByRole('button', { name: kind === 'sign-up' ? 'get started' : 'continue securely →', exact: true }).click();
  const authorizeResponse = await authorization;
  expect(authorizeResponse.status(), 'app starts a real authorization flow').toBe(200);
  account.apiOrigin = new URL(authorizeResponse.url()).origin;
  await page.waitForURL((url) => url.hostname.endsWith('.authkit.app'));
  // The app's Get started action uses AuthKit's passwordless entry form.
  // Verifying an unknown email creates the account; an existing email signs in.
  const since = new Date(Date.now() - 2000).toISOString();
  await page.getByRole('textbox', { name: 'Email', exact: true }).fill(account.email);
  await page.getByRole('button', { name: 'Continue with email', exact: true }).click();
  await expect(page.getByRole('textbox', { name: 'One-time code', exact: true })).toBeVisible();
  const code = await verificationCode(account, since);
  const exchange = page.waitForResponse((response) =>
    new URL(response.url()).pathname === '/v1/auth/exchange' && response.request().method() === 'POST');
  await page.getByRole('textbox', { name: 'One-time code', exact: true }).fill(code);
  expect((await exchange).status(), 'real WorkOS code exchanges for a web session').toBe(204);
  await page.waitForURL((url) => url.origin === appOrigin && !url.pathname.startsWith('/auth/'));
  await expect.poll(async () => (await browserMe(page, account)).status).toBe(200);
  const profile = (await browserMe(page, account)).user!;
  expect(profile.email).toBe(account.email);
  const provider = await findProviderUser(account);
  expect(provider?.email).toBe(account.email);
  account.providerId = provider!.id;
  return profile;
}

async function completeOnboarding(page: Page) {
  await page.getByRole('button', { name: 'Job interviews', exact: true }).click();
  await page.getByRole('button', { name: 'continue →', exact: true }).click();
  await page.getByRole('button', { name: 'Negativity', exact: true }).click();
  await page.getByRole('button', { name: 'continue →', exact: true }).click();
  await page.getByText('X', { exact: true }).click();
  await page.getByRole('checkbox').click();
  await page.getByRole('button', { name: 'continue →', exact: true }).click();
  await page.getByRole('button', { name: 'upload archives →', exact: true }).click();
  await expect(page).toHaveURL((url) => url.origin === appOrigin && url.pathname === '/scan');
}

async function openProfile(page: Page) {
  await page.goto('/profile');
  await expect(page.getByRole('button', { name: 'delete Ghostpost account', exact: true })).toBeVisible();
}

async function screenshot(page: Page, info: TestInfo, name: string) {
  const path = info.outputPath(`${name}.png`);
  await page.screenshot({ path, fullPage: true });
  await info.attach(name, { path, contentType: 'image/png' });
}

async function providerIsGone(account: Account) {
  if (!account.providerId) return !(await findProviderUser(account));
  return (await workos(`/user_management/users/${account.providerId}`)).status === 404;
}

async function cleanup(account: Account) {
  const user = await findProviderUser(account);
  if (!user) return;
  account.providerId = user.id;
  for (const context of account.contexts) {
    if (!account.apiOrigin) break;
    const me = await context.request.get(`${account.apiOrigin}/v1/me`);
    if (me.status() !== 200 || profileFrom(await me.json()).email !== account.email) continue;
    const csrf = await context.request.get(`${account.apiOrigin}/v1/auth/csrf`);
    expect(csrf.status(), 'cleanup obtains a real session-bound CSRF token').toBe(200);
    const deleted = await context.request.delete(`${account.apiOrigin}/v1/me`, {
      headers: {
        Origin: appOrigin,
        'Sec-Fetch-Site': account.apiOrigin === appOrigin ? 'same-origin' : 'same-site',
        'Sec-Fetch-Mode': 'cors',
        'X-CSRF-Token': string(object(await csrf.json()).token),
        'Idempotency-Key': randomUUID(),
      },
    });
    expect(deleted.status(), 'cleanup schedules deletion through the real app').toBe(202);
    account.evidence.cleanup = 'app deletion requested';
    await expect.poll(() => providerIsGone(account), { timeout: 120_000, intervals: [1000, 2000, 5000] }).toBe(true);
    return;
  }
  // Failed logins may create a WorkOS user before an app session exists. Removing
  // only this exact generated identity also emits user.deleted to the app webhook.
  expect(user.email).toBe(account.email);
  await workos(`/user_management/users/${user.id}`, 'DELETE');
  account.evidence.cleanup = 'provider-only identity removed after missing app session';
}

const test = base.extend<{ account: Account }>({
  account: [async ({ context }, use, info) => {
    const account: Account = {
      email: `ghostpost-e2e-${randomUUID()}@example.com`,
      startedAt: new Date().toISOString(),
      contexts: [context],
      evidence: { scenario: info.title, browser: info.project.name, appOrigin, checks: {} },
    };
    try {
      await use(account);
    } finally {
      try {
        await cleanup(account);
        account.evidence.providerIdentityRemoved = await providerIsGone(account);
      } catch (error) {
        account.evidence.cleanupError = error instanceof Error ? error.message : String(error);
        throw error;
      } finally {
        account.evidence.finishedAt = new Date().toISOString();
        account.evidence.email = account.email;
        account.evidence.appUserId = account.profile?.id;
        account.evidence.status = account.evidence.cleanupError ? 'failed' : info.status;
        const path = info.outputPath('journey.json');
        await writeFile(path, JSON.stringify(account.evidence, null, 2));
        await info.attach('journey', { path, contentType: 'application/json' });
        for (const extra of account.contexts.slice(1)) await extra.close();
      }
    }
  }, { timeout: 180_000 }],
});

test('sign up creates an authenticated account and survives reload', async ({ page, account }, info) => {
  expect(await findProviderUser(account)).toBeUndefined();
  account.profile = await authenticate(page, account, 'sign-up');
  await expect(page).toHaveURL(/\/onboarding\/coming-up$/);
  await page.reload();
  await expect(page.getByRole('button', { name: 'Job interviews', exact: true })).toBeVisible();
  const persisted = await browserMe(page, account);
  expect(persisted.status).toBe(200);
  expect(persisted.user?.id).toBe(account.profile.id);
  account.evidence.checks = { newAccount: true, callbackSession: 200, sameIdentityAfterReload: true };
  await screenshot(page, info, 'signed-up');
});

test('sign in returns to the same existing account and saved onboarding', async ({ browser, page, account }, info) => {
  const seed = await browser.newContext({ baseURL: appOrigin });
  account.contexts.push(seed);
  const seedPage = await seed.newPage();
  account.profile = await authenticate(seedPage, account, 'sign-up');
  await completeOnboarding(seedPage);

  const returning = await authenticate(page, account, 'sign-in');
  expect(returning.id).toBe(account.profile.id);
  await expect(page).toHaveURL((url) => url.origin === appOrigin && url.pathname === '/scan');
  await openProfile(page);
  await expect(page.getByText(account.email, { exact: true }).first()).toBeVisible();
  await page.reload();
  await expect(page.getByText(account.email, { exact: true }).first()).toBeVisible();
  expect((await browserMe(page, account)).user?.id).toBe(account.profile.id);
  account.evidence.checks = { existingIdentityPreserved: true, completedOnboardingPreserved: true, sessionAfterReload: 200 };
  await screenshot(page, info, 'signed-in');
});

test('log out revokes the server session and stays logged out after reload', async ({ browser, page, account }, info) => {
  account.profile = await authenticate(page, account, 'sign-up');
  await completeOnboarding(page);
  // Independent live session is kept for cleanup; the original cookie copy is
  // used below to catch a logout that only clears browser-local state.
  const cleanupContext = await browser.newContext({ baseURL: appOrigin });
  account.contexts.push(cleanupContext);
  await authenticate(await cleanupContext.newPage(), account, 'sign-in');
  const staleSession = await browser.newContext({ storageState: await page.context().storageState() });
  account.contexts.push(staleSession);
  await openProfile(page);
  page.once('dialog', (dialog) => dialog.accept());
  const response = page.waitForResponse((r) => new URL(r.url()).pathname === '/v1/auth/logout' && r.request().method() === 'POST');
  await page.getByRole('button', { name: 'log out', exact: true }).click();
  expect((await response).status()).toBe(204);
  await expect(page.getByRole('button', { name: 'get started', exact: true })).toBeVisible();
  const stale = await staleSession.request.get(`${account.apiOrigin}/v1/me`);
  expect(stale.status(), 'saved pre-logout cookie is revoked server-side').toBe(401);
  await page.reload();
  await expect(page.getByRole('button', { name: 'get started', exact: true })).toBeVisible();
  expect((await browserMe(page, account)).status).toBe(401);
  account.evidence.checks = { logout: 204, oldSessionRejected: 401, loggedOutAfterReload: true };
  await screenshot(page, info, 'logged-out');
});

test('sign up then delete account revokes every session and purges WorkOS', async ({ browser, page, account }, info) => {
  account.profile = await authenticate(page, account, 'sign-up');
  await completeOnboarding(page);
  const otherSession = await browser.newContext({ baseURL: appOrigin });
  account.contexts.push(otherSession);
  const otherUser = await authenticate(await otherSession.newPage(), account, 'sign-in');
  expect(otherUser.id).toBe(account.profile.id);
  await openProfile(page);
  page.once('dialog', (dialog) => dialog.accept());
  const response = page.waitForResponse((r) => new URL(r.url()).pathname === '/v1/me' && r.request().method() === 'DELETE');
  await page.getByRole('button', { name: 'delete Ghostpost account', exact: true }).click();
  expect((await response).status()).toBe(202);
  await expect(page.getByRole('button', { name: 'get started', exact: true })).toBeVisible();
  expect((await otherSession.request.get(`${account.apiOrigin}/v1/me`)).status(), 'deletion revokes other live sessions').toBe(401);
  await page.reload();
  await expect(page.getByRole('button', { name: 'get started', exact: true })).toBeVisible();
  expect((await browserMe(page, account)).status).toBe(401);
  await expect.poll(() => providerIsGone(account), { message: 'real purge worker deletes the WorkOS identity', timeout: 120_000, intervals: [1000, 2000, 5000] }).toBe(true);
  account.evidence.checks = { deletion: 202, otherSessionRevoked: 401, deletedAfterReload: true, providerIdentityRemoved: true };
  await screenshot(page, info, 'account-deleted');
});
