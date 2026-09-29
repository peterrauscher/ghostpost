import type { APIRoute } from 'astro';
import { absoluteUrl } from '../lib/site';

// AI crawlers are welcome: being cited by answer engines is part of discoverability.
export const GET: APIRoute = () =>
  new Response(`User-agent: *\nAllow: /\nDisallow: /api/\n\nSitemap: ${absoluteUrl('/sitemap.xml')}\n`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
