import { parseContact } from '../src/lib/contact';

const SOURCES: Record<string, true> = { hero: true, cta: true };

type Outcome = { status: 200 } | { status: 400 | 429; error: string };

/**
 * Only `/api/*` reaches this Worker (see `assets.run_worker_first`); everything else is served by the
 * static-assets layer without invoking it.
 */
export default {
  async fetch(request, env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname !== '/api/waitlist') return env.ASSETS.fetch(request);
    if (request.method !== 'POST') {
      return Response.json({ ok: false, error: 'method not allowed' }, { status: 405, headers: { Allow: 'POST' } });
    }

    const outcome = await joinWaitlist(request, env);
    // fetch() from the page asks for JSON; a plain no-JS form post gets redirected to a static page.
    if (request.headers.get('Accept')?.includes('application/json')) {
      const body = outcome.status === 200 ? { ok: true } : { ok: false, error: outcome.error };
      return Response.json(body, { status: outcome.status, headers: { 'Cache-Control': 'no-store' } });
    }
    return Response.redirect(new URL(outcome.status === 200 ? '/joined/' : '/oops/', url).href, 303);
  },
} satisfies ExportedHandler<Env>;

async function joinWaitlist(request: Request, env: Env): Promise<Outcome> {
  // CF-Connecting-IP is set by Cloudflare's edge and cannot be forged by clients in production.
  const ip = request.headers.get('CF-Connecting-IP') ?? 'unknown';
  if (!(await env.WAITLIST_LIMITER.limit({ key: ip })).success) {
    return { status: 429, error: 'too many tries. give it a minute and try again.' };
  }

  let form: FormData;
  try {
    form = await request.formData();
  } catch {
    return { status: 400, error: 'that request didn’t look right. try again?' };
  }

  // Honeypot: humans never see this field. Pretend it worked so bots learn nothing.
  if (String(form.get('company') ?? '') !== '') return { status: 200 };

  const contact = parseContact(String(form.get('contact') ?? ''));
  if (!contact) return { status: 400, error: 'that doesn’t look like an email or phone number.' };

  const source = String(form.get('source') ?? '');
  // Re-joining is a silent no-op, so the response never reveals whether someone is already on the list.
  await env.DB.prepare('INSERT INTO waitlist (contact, kind, source) VALUES (?1, ?2, ?3) ON CONFLICT (contact) DO NOTHING')
    .bind(contact.value, contact.kind, Object.hasOwn(SOURCES, source) ? source : null)
    .run();
  return { status: 200 };
}
