/**
 * Renders the 1200×630 social share card to public/og.png with the site font and mascot.
 * Rerun after changing copy or branding: `bun run og`.
 */
import { readFileSync } from 'node:fs';
import { chromium } from '@playwright/test';

const dataUrl = (path: string, type: string) =>
  `data:${type};base64,${readFileSync(new URL(path, import.meta.url)).toString('base64')}`;
const font = dataUrl('../src/fonts/sn-pro-latin.woff2', 'font/woff2');
const ghost = dataUrl('../../app/assets/images/logo-trim.png', 'image/png');

const html = `<!doctype html><html><head><style>
  @font-face { font-family: 'SN Pro'; src: url(${font}) format('woff2'); font-weight: 100 900; }
  * { margin: 0; box-sizing: border-box; }
  body {
    width: 1200px; height: 630px; overflow: hidden; font-family: 'SN Pro', sans-serif; color: #171717;
    background:
      radial-gradient(closest-side at 78% 42%, rgb(167 139 250 / 0.35), transparent),
      radial-gradient(closest-side at 20% 90%, rgb(255 214 236 / 0.7), transparent),
      linear-gradient(180deg, #f7f1ff 0%, #f0e6ff 55%, #ede4ff 100%);
    display: flex; align-items: center; padding: 0 88px; gap: 40px;
  }
  .copy { flex: 1; }
  .brand { display: flex; align-items: center; gap: 14px; font-size: 34px; font-weight: 800; letter-spacing: -0.03em; color: #5925be; }
  .brand img { width: 44px; }
  h1 { margin-top: 36px; font-size: 84px; line-height: 1.0; font-weight: 800; letter-spacing: -0.045em; }
  h1 span { display: block; background: linear-gradient(100deg, #7f4ae0 10%, #5925be 60%, #8b6ae8 100%); -webkit-background-clip: text; color: transparent; }
  p { margin-top: 28px; max-width: 600px; font-size: 28px; line-height: 1.35; color: #6b6578; font-weight: 500; }
  .mascot { width: 330px; filter: drop-shadow(0 30px 50px rgb(89 37 190 / 0.25)); rotate: 6deg; }
</style></head><body>
  <div class="copy">
    <div class="brand"><img src="${ghost}" alt="" />ghostpost</div>
    <h1>let’s clean <span>your slate.</span></h1>
    <p>find the posts that could hold you back, before admissions, recruiters, or rush see them.</p>
  </div>
  <img class="mascot" src="${ghost}" alt="" />
</body></html>`;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 630 } });
await page.setContent(html);
await page.evaluate(() => document.fonts.ready);
await page.screenshot({ path: new URL('../public/og.png', import.meta.url).pathname });
await browser.close();
