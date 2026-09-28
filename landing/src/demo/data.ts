// Sample content for the in-page app demo. Shapes follow app/src/domain/types.ts (FlaggedPost).
export type Risk = 'high' | 'medium' | 'low';
export type Platform = 'instagram' | 'tiktok' | 'x' | 'facebook' | 'reddit';

export interface DemoPost {
  id: string;
  platform: Platform;
  platformLabel: string;
  date: string;
  risk: Risk;
  category: string;
  quote: string;
  explanation: string;
  whyFlagged: string;
  likes: number;
  comments: number;
}

export const PLATFORMS: readonly { id: Platform; label: string }[] = [
  { id: 'instagram', label: 'Instagram' },
  { id: 'tiktok', label: 'TikTok' },
  { id: 'x', label: 'X' },
  { id: 'facebook', label: 'Facebook' },
  { id: 'reddit', label: 'Reddit' },
];

export const DEMO_POSTS: readonly DemoPost[] = [
  {
    id: 'p1',
    platform: 'x',
    platformLabel: 'X',
    date: 'Mar 2021',
    risk: 'high',
    category: 'Drinking / drugs',
    quote: 'still recovering from saturday 🍻🍻 never drinking again (lie)',
    explanation: 'Jokes about heavy drinking can read as poor judgment to someone who doesn’t know you.',
    whyFlagged:
      'Mentions drinking in a way that sounds excessive. Admissions teams and employers often notice substance references, even obvious jokes.',
    likes: 48,
    comments: 12,
  },
  {
    id: 'p2',
    platform: 'instagram',
    platformLabel: 'Instagram',
    date: 'Aug 2022',
    risk: 'medium',
    category: 'Negativity',
    quote: 'my manager is genuinely the worst, counting down the days 🙃',
    explanation: 'Bad-mouthing a boss is one of the first things recruiters look for.',
    whyFlagged:
      'Complaining about a current or past employer is a common reason hiring managers pass on a candidate.',
    likes: 131,
    comments: 27,
  },
  {
    id: 'p3',
    platform: 'tiktok',
    platformLabel: 'TikTok',
    date: 'Jan 2023',
    risk: 'medium',
    category: 'Inappropriate language',
    quote: 'this song goes so f***ing hard i can’t even',
    explanation: 'Strong language is fine with friends, but can look unprofessional out of context.',
    whyFlagged: 'Contains profanity in a public caption. Out of context, it can read as careless.',
    likes: 902,
    comments: 64,
  },
  {
    id: 'p4',
    platform: 'reddit',
    platformLabel: 'Reddit',
    date: 'Nov 2020',
    risk: 'low',
    category: 'Political takes',
    quote: 'honestly anyone who votes the other way is just clueless',
    explanation: 'Heated political posts can create friction with reviewers who see it differently.',
    whyFlagged: 'Dismisses people with different political views. Low risk, but worth a second look.',
    likes: 15,
    comments: 41,
  },
];

const riskOrder: Risk[] = ['high', 'medium', 'low'];
const sweepWeight: Record<Risk, number> = { high: 95, medium: 60, low: 30 };

/** Overall risk for a set of remaining flags: the worst remaining level, gauge sweep in degrees. */
export function overallRisk(posts: readonly DemoPost[]): { level: Risk | 'clear'; sweep: number } {
  const level = riskOrder.find((risk) => posts.some((post) => post.risk === risk)) ?? 'clear';
  const sweep = Math.min(300, posts.reduce((sum, post) => sum + sweepWeight[post.risk], 0));
  return { level, sweep };
}

export function auditHeadline(count: number): string {
  if (count === 0) return 'your slate is clean. nice work ✨';
  if (count === 1) return 'we found 1 post that could raise a red flag';
  return `we found ${count} posts that could raise red flags`;
}

export const riskLabel = (level: Risk | 'clear') => (level === 'clear' ? 'Clear' : level[0].toUpperCase() + level.slice(1));
