import type { APIRoute, GetStaticPaths } from 'astro';
import { ICON_SIZES, renderBrandIcon } from '../../lib/brand-icon';

// /icons/icon-{size}.png (rounded tile) and /icons/apple-touch-icon.png (full bleed, iOS rounds it).
export const getStaticPaths = (() => [
  ...ICON_SIZES.map((size) => ({ params: { name: `icon-${size}` }, props: { size, fullBleed: false } })),
  { params: { name: 'apple-touch-icon' }, props: { size: 180, fullBleed: true } },
]) satisfies GetStaticPaths;

export const GET: APIRoute = async ({ props }) => {
  const png = await renderBrandIcon(props.size, { fullBleed: props.fullBleed });
  return new Response(new Uint8Array(png), { headers: { 'Content-Type': 'image/png' } });
};
