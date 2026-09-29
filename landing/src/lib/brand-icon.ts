import { resolve } from 'node:path';
import sharp from 'sharp';

// Builds run from landing/, so the app's mascot is one directory up.
const GHOST = resolve(process.cwd(), '../app/assets/images/logo-trim.png');

export const ICON_SIZES = [32, 48, 192, 512] as const;
export type IconSize = (typeof ICON_SIZES)[number];

/**
 * Renders the header logo tile (Logo.astro): white ghost on the purple gradient, 1/3 corner radius,
 * ghost at 2/3 of the tile. `fullBleed` drops the rounded corners for iOS, which masks icons itself.
 */
export async function renderBrandIcon(size: number, { fullBleed = false } = {}): Promise<Buffer> {
  const radius = fullBleed ? 0 : size / 3;
  const tile = Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}">
      <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#a78bfa"/><stop offset="1" stop-color="#7f4ae0"/>
      </linearGradient></defs>
      <rect width="${size}" height="${size}" rx="${radius}" fill="url(#g)"/>
    </svg>`,
  );
  const ghostSize = Math.round((size * 2) / 3);
  const ghost = await sharp(GHOST).resize(ghostSize, ghostSize, { fit: 'contain', background: { r: 0, g: 0, b: 0, alpha: 0 } }).toBuffer();
  return sharp(tile)
    .composite([{ input: ghost, gravity: 'center' }])
    .png({ compressionLevel: 9 })
    .toBuffer();
}
