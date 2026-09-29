import type { APIRoute } from 'astro';
import { absoluteUrl, SITE } from '../lib/site';

// https://llmstxt.org: H1 name, blockquote summary, free-form details, then H2 link lists.
export const GET: APIRoute = () =>
  new Response(
    `# ${SITE.name}

> ${SITE.description}

Ghostpost is a mobile app for students and early-career people, currently in pre-launch with a waitlist.

- **Connect:** users upload the data export zip from Instagram, TikTok, X, Facebook, and Reddit. No social passwords, and no linking of live accounts.
- **Scan:** every post is checked for what admissions officers, recruiters, and fraternity/sorority rush chairs tend to notice (language, drinking, hot takes, and more), tuned to what's coming up for the user. Every flag comes with a reason.
- **Review:** one overall risk level plus a list of flagged posts sorted from high to low risk.
- **Clean up:** the user decides, flag by flag, to delete, archive, or keep each post.
- **Privacy:** uploads stay private. Ghostpost never posts, edits, or deletes anything on live accounts, and users can delete their account at any time.

Why it matters: in a 2018 CareerBuilder/Harris Poll survey, 70% of employers said they research candidates on social media, and 57% of those found content that made them pass on a hire.

## Pages

- [Home](${absoluteUrl('/')}): what Ghostpost does, how it works, privacy, and the waitlist signup
- [Join the waitlist](${absoluteUrl('/#join')}): sign up with an email or US/Canada phone number for early access

## Optional

- [Sitemap](${absoluteUrl('/sitemap.xml')})
- [CareerBuilder hiring survey (2018)](https://www.prnewswire.com/news-releases/more-than-half-of-employers-have-found-content-on-social-media-that-caused-them-not-to-hire-a-candidate-according-to-recent-careerbuilder-survey-300694437.html)
`,
    { headers: { 'Content-Type': 'text/markdown; charset=utf-8' } },
  );
