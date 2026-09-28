import { fileURLToPath } from 'node:url';
import { defineConfig, envField } from 'astro/config';

export default defineConfig({
  // Every page is prerendered to plain files; Cloudflare serves them as static assets at the edge.
  output: 'static',
  build: {
    // Inline the (small) stylesheet so first paint never waits on a CSS request.
    inlineStylesheets: 'always',
  },
  env: {
    schema: {
      // Canonical origin of this landing page, used for canonical + Open Graph URLs.
      PUBLIC_SITE_URL: envField.string({ context: 'client', access: 'public', url: true }),
    },
  },
  vite: {
    resolve: {
      // Mascots and platform icons live with the Expo app; the landing page reuses them as the single source.
      alias: { '@app-assets': fileURLToPath(new URL('../app/assets', import.meta.url)) },
    },
  },
});
