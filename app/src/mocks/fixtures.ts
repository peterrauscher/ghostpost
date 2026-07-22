import type {
  DashboardData,
  FlaggedPost,
  OnboardingAnswers,
  ReviewListData,
  ScanStatus,
  UserProfile,
} from '@/domain/types';

export const MOCK_USER: UserProfile = {
  id: 'user_jordan',
  name: 'Jordan Lee',
  greetingName: 'jordan',
};

export const MOCK_FLAGGED_POSTS: FlaggedPost[] = [
  {
    id: 'post_1',
    platform: 'instagram',
    platformLabel: 'Instagram',
    date: 'Apr 12, 2023',
    quote: '“drunk nights >”',
    risk: 'high',
    category: 'substances',
    tags: ['alcohol'],
    engagementLabel: '◎ 1.2K',
    likes: 142,
    comments: 23,
    explanation: 'This post may raise concerns about alcohol use.',
    whyFlagged:
      'Mentions of alcohol can be a red flag in college, scholarship, and job applications.',
    status: 'open',
  },
  {
    id: 'post_2',
    platform: 'x',
    platformLabel: 'X (Twitter)',
    date: 'Jan 8, 2024',
    quote: '“these ppl are so dumb 🙄”',
    risk: 'medium',
    category: 'language',
    tags: ['negative language'],
    engagementLabel: '◎ 234',
    likes: 34,
    comments: 12,
    explanation: 'This post uses dismissive language that can read as unprofessional.',
    whyFlagged: 'Negative or insulting language can hurt first impressions with admissions and employers.',
    status: 'open',
  },
  {
    id: 'post_3',
    platform: 'tiktok',
    platformLabel: 'TikTok',
    date: 'Mar 3, 2023',
    quote: '“not trying in class lol”',
    risk: 'medium',
    category: 'professionalism',
    tags: ['lack of professionalism'],
    engagementLabel: '◎ 3.7K',
    likes: 890,
    comments: 41,
    explanation: 'This post may signal a lack of seriousness about school or work.',
    whyFlagged: 'Jokes about not trying can raise concerns for colleges and internships.',
    status: 'open',
  },
  {
    id: 'post_4',
    platform: 'instagram',
    platformLabel: 'Instagram',
    date: 'May 21, 2022',
    quote: '“throwback to summer ☀️”',
    risk: 'low',
    category: 'parties',
    tags: ['inappropriate party'],
    engagementLabel: '◎ 812',
    likes: 96,
    comments: 8,
    explanation: 'This post may include party context that some reviewers notice.',
    whyFlagged: 'Party-related content can be interpreted differently depending on the audience.',
    status: 'open',
  },
  {
    id: 'post_5',
    platform: 'tiktok',
    platformLabel: 'TikTok',
    date: 'Jun 2, 2023',
    quote: '“nobody needs to know 👀”',
    risk: 'medium',
    category: 'language',
    tags: ['suggestive'],
    engagementLabel: '◎ 2.1K',
    likes: 410,
    comments: 19,
    explanation: 'Suggestive phrasing can create ambiguity for reviewers.',
    whyFlagged: 'Ambiguous or suggestive posts can create unnecessary risk.',
    status: 'open',
  },
  {
    id: 'post_6',
    platform: 'x',
    platformLabel: 'X (Twitter)',
    date: 'Nov 4, 2022',
    quote: '“hot take on the election…”',
    risk: 'high',
    category: 'politics',
    tags: ['politics'],
    engagementLabel: '◎ 980',
    likes: 120,
    comments: 67,
    explanation: 'Political takes may be scrutinized by admissions or employers.',
    whyFlagged: 'Strong political opinions can distract from your application narrative.',
    status: 'open',
  },
  {
    id: 'post_7',
    platform: 'instagram',
    platformLabel: 'Instagram',
    date: 'Aug 19, 2021',
    quote: '“that party was insane…”',
    risk: 'high',
    category: 'substances',
    tags: ['substances'],
    engagementLabel: '◎ 4.4K',
    likes: 520,
    comments: 33,
    explanation: 'Party content with suggestive framing can raise substance concerns.',
    whyFlagged: 'High-visibility party posts are commonly flagged for review.',
    status: 'open',
  },
];

export const DEFAULT_ONBOARDING: OnboardingAnswers = {
  comingUp: [],
  concerns: [],
  platforms: [],
};

export function buildDashboard(unlocked: boolean): DashboardData {
  const openPosts = MOCK_FLAGGED_POSTS.filter((p) => p.status === 'open');
  return {
    user: MOCK_USER,
    auditHeadline: `we found ${openPosts.length} posts that could raise red flags`,
    focusAreas: [
      { id: 'colleges', label: 'colleges', symbol: '🎓' },
      { id: 'fraternities', label: 'fraternities', symbol: '⌂' },
      { id: 'internships', label: 'internships / jobs', symbol: '💼' },
    ],
    flaggedPreview: openPosts.slice(0, 3),
    risk: {
      level: 'medium',
      flaggedCount: openPosts.length,
      gaugeSweep: 245,
    },
    unlocked,
  };
}

export function buildReviewList(): ReviewListData {
  const posts = MOCK_FLAGGED_POSTS.filter((p) => p.status === 'open');
  return {
    posts,
    filters: {
      all: posts.length,
      high: posts.filter((p) => p.risk === 'high').length,
      medium: posts.filter((p) => p.risk === 'medium').length,
      low: posts.filter((p) => p.risk === 'low').length,
    },
  };
}

export function buildScanStatus(progress: number): ScanStatus {
  if (progress < 0.34) {
    return { phase: 'connecting', progress, message: 'connecting...' };
  }
  if (progress < 0.67) {
    return { phase: 'scanning', progress, message: 'scanning...' };
  }
  if (progress < 1) {
    return { phase: 'flagging', progress, message: 'flagging...' };
  }
  return { phase: 'complete', progress: 1, message: 'scan complete' };
}
