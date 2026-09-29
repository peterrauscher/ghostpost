import type { APIRoute } from 'astro';
import { getImage } from 'astro:assets';
import appIcon from '@app-assets/images/logo.png';
import { SITE } from '../lib/site';

export const GET: APIRoute = async () => {
  const icons = await Promise.all(
    [192, 512].map(async (size) => {
      const image = await getImage({ src: appIcon, width: size, height: size, format: 'png' });
      return { src: image.src, sizes: `${size}x${size}`, type: 'image/png' };
    }),
  );
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
