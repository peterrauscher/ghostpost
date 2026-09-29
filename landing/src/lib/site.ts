import { PUBLIC_SITE_URL } from 'astro:env/client';

/** Single source for the copy search engines, social cards, and LLM agents see. */
export const SITE = {
  name: 'Ghostpost',
  title: 'Ghostpost: Clean Up Your Social Media Before It Counts',
  description:
    'Ghostpost scans your Instagram, TikTok, X, Facebook, and Reddit history for posts that could hurt you with college admissions, recruiters, or rush.',
  shareImage: { path: '/og.png', width: 1200, height: 630, alt: 'Ghostpost ghost mascot: let’s clean your slate.' },
  themeColor: '#F7F1FF',
} as const;

/** Indexable pages, listed in the sitemap and llms.txt. Utility pages (404, confirmations) are noindex and excluded. */
export const INDEXABLE_PATHS = ['/'] as const;

export const absoluteUrl = (path: string) => new URL(path, PUBLIC_SITE_URL).href;
