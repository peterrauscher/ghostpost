import {
  DEFAULT_ONBOARDING,
  MOCK_FLAGGED_POSTS,
  MOCK_USER,
  buildDashboard,
  buildScanStatus,
} from '@/mocks/fixtures';
import type {
  DashboardData,
  FlaggedPost,
  OnboardingAnswers,
  ReviewAction,
  ReviewListData,
  ScanStatus,
  UserProfile,
} from '@/domain/types';

export interface GhostpostApi {
  getProfile(): Promise<UserProfile>;
  getDashboard(): Promise<DashboardData>;
  getReviewList(risk?: string): Promise<ReviewListData>;
  getFlaggedPost(id: string): Promise<FlaggedPost>;
  submitOnboarding(answers: OnboardingAnswers): Promise<{ ok: true }>;
  startScan(): Promise<ScanStatus>;
  getScanStatus(): Promise<ScanStatus>;
  unlockHome(): Promise<DashboardData>;
  applyReviewAction(id: string, action: ReviewAction): Promise<FlaggedPost>;
  resetDemo(): Promise<void>;
}

const delay = (ms = 350) => new Promise((resolve) => setTimeout(resolve, ms));

let onboarding: OnboardingAnswers = { ...DEFAULT_ONBOARDING };
let unlocked = false;
let scanProgress = 0;
let posts = MOCK_FLAGGED_POSTS.map((p) => ({ ...p }));

function currentDashboard(isUnlocked: boolean): DashboardData {
  const openPosts = posts.filter((p) => p.status === 'open');
  return {
    ...buildDashboard(isUnlocked),
    auditHeadline: `we found ${openPosts.length} posts that could raise red flags`,
    flaggedPreview: openPosts.slice(0, 3),
    risk: {
      level: 'medium',
      flaggedCount: openPosts.length,
      gaugeSweep: 245,
    },
    unlocked: isUnlocked,
  };
}

export const mockApi: GhostpostApi = {
  async getProfile() {
    await delay();
    return MOCK_USER;
  },

  async getDashboard() {
    await delay();
    return currentDashboard(unlocked);
  },

  async getReviewList(risk) {
    await delay();
    const open = posts.filter((p) => p.status === 'open');
    const filtered =
      !risk || risk === 'all' ? open : open.filter((p) => p.risk === risk);
    return {
      posts: filtered,
      filters: {
        all: open.length,
        high: open.filter((p) => p.risk === 'high').length,
        medium: open.filter((p) => p.risk === 'medium').length,
        low: open.filter((p) => p.risk === 'low').length,
      },
    };
  },

  async getFlaggedPost(id) {
    await delay();
    const post = posts.find((p) => p.id === id);
    if (!post) throw new Error(`Post ${id} not found`);
    return { ...post };
  },

  async submitOnboarding(answers) {
    await delay(500);
    onboarding = {
      comingUp: [...answers.comingUp],
      concerns: [...answers.concerns],
      platforms: [...answers.platforms],
    };
    scanProgress = 0;
    unlocked = false;
    return { ok: true as const };
  },

  async startScan() {
    await delay(200);
    scanProgress = 0.1;
    return buildScanStatus(scanProgress);
  },

  async getScanStatus() {
    await delay(180);
    if (scanProgress < 1) {
      scanProgress = Math.min(1, scanProgress + 0.18);
    }
    return buildScanStatus(scanProgress);
  },

  async unlockHome() {
    await delay(400);
    unlocked = true;
    return currentDashboard(true);
  },

  async applyReviewAction(id, action) {
    await delay(300);
    const index = posts.findIndex((p) => p.id === id);
    if (index < 0) throw new Error(`Post ${id} not found`);
    const nextStatus =
      action === 'delete'
        ? 'deleted'
        : action === 'archive'
          ? 'archived'
          : action === 'keep'
            ? 'kept'
            : 'resolved';
    posts[index] = { ...posts[index], status: nextStatus };
    return { ...posts[index] };
  },

  async resetDemo() {
    await delay(200);
    onboarding = { ...DEFAULT_ONBOARDING };
    unlocked = false;
    scanProgress = 0;
    posts = MOCK_FLAGGED_POSTS.map((p) => ({ ...p }));
  },
};

/** Exposed for debugging / profile placeholder. */
export function getMockOnboardingSnapshot() {
  return onboarding;
}
