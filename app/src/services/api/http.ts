import type { GhostpostApi } from './mock';
import type {
  DashboardData,
  FlaggedPost,
  OnboardingAnswers,
  ReviewAction,
  ReviewListData,
  ScanStatus,
  UserProfile,
} from '@/domain/types';

function getBaseUrl() {
  return process.env.EXPO_PUBLIC_API_URL?.replace(/\/$/, '') ?? '';
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const base = getBaseUrl();
  if (!base) {
    throw new Error('EXPO_PUBLIC_API_URL is not configured');
  }

  const response = await fetch(`${base}${path}`, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Request failed: ${response.status}`);
  }

  return response.json() as Promise<T>;
}

export const httpApi: GhostpostApi = {
  getProfile: () => request<UserProfile>('/me'),
  getDashboard: () => request<DashboardData>('/dashboard'),
  getReviewList: (risk) =>
    request<ReviewListData>(`/review${risk && risk !== 'all' ? `?risk=${risk}` : ''}`),
  getFlaggedPost: (id) => request<FlaggedPost>(`/posts/${id}`),
  submitOnboarding: (answers: OnboardingAnswers) =>
    request<{ ok: true }>('/onboarding', {
      method: 'POST',
      body: JSON.stringify(answers),
    }),
  startScan: () => request<ScanStatus>('/scan', { method: 'POST' }),
  getScanStatus: () => request<ScanStatus>('/scan'),
  unlockHome: () => request<DashboardData>('/home/unlock', { method: 'POST' }),
  applyReviewAction: (id: string, action: ReviewAction) =>
    request<FlaggedPost>(`/posts/${id}/actions`, {
      method: 'POST',
      body: JSON.stringify({ action }),
    }),
  resetDemo: () => request<void>('/demo/reset', { method: 'POST' }),
};
