import type { APIRoute } from 'astro';
import { SITE } from '../lib/site';

export const GET: APIRoute = () => {
  const icons = [192, 512].map((size) => ({ src: `/icons/icon-${size}.png`, sizes: `${size}x${size}`, type: 'image/png' }));
  return Response.json(
    {
      name: SITE.name,
      short_name: SITE.name,
      description: SITE.description,
      start_url: '/',
      display: 'browser',
      background_color: SITE.themeColor,
      theme_color: SITE.themeColor,
      icons,
    },
    { headers: { 'Content-Type': 'application/manifest+json' } },
  );
};
