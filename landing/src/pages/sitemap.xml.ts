import type { APIRoute } from 'astro';
import { absoluteUrl, INDEXABLE_PATHS } from '../lib/site';

const lastmod = new Date().toISOString().slice(0, 10);

export const GET: APIRoute = () =>
  new Response(
    `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${INDEXABLE_PATHS.map((path) => `  <url><loc>${absoluteUrl(path)}</loc><lastmod>${lastmod}</lastmod></url>`).join('\n')}
</urlset>
`,
    { headers: { 'Content-Type': 'application/xml; charset=utf-8' } },
  );
