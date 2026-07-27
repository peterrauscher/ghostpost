import { apiRequest } from './client';
import type { GhostpostApi, NativeExchangeResponse, PlatformCatalog, ArchiveImport, ArchiveImportList, Entitlement, ReviewListQuery, ReserveArchiveResponse } from './types';
import type { DashboardData, FlaggedPost, ReviewListData, ScanStatus, UserProfile } from '@/domain/types';
import type { OnboardingState } from '@/domain/onboarding';

const query = (values: object) => {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) if (typeof value === 'string' || typeof value === 'number') params.set(key, String(value));
  const encoded = params.toString();
  return encoded ? `?${encoded}` : '';
};

export const httpApi: GhostpostApi = {
  authorize: (client) => apiRequest(`/v1/auth/authorize?client=${client}`, { skipAuthRefresh: true }),
  exchangeWeb: (code, state) => apiRequest<void>('/v1/auth/exchange', { method: 'POST', body: JSON.stringify({ client: 'web', code, state }), skipAuthRefresh: true }),
  exchangeNative: (code, state, exchangeSecret) => apiRequest<NativeExchangeResponse>('/v1/auth/exchange', { method: 'POST', body: JSON.stringify({ client: 'native', code, state, exchangeSecret }), skipAuthRefresh: true }),
  logout: () => apiRequest<void>('/v1/auth/logout', { method: 'POST' }),
  getProfile: async () => (await apiRequest<{ user: UserProfile }>('/v1/me')).user,
  getOnboarding: () => apiRequest<OnboardingState>('/v1/me/onboarding'),
  putOnboarding: (state, idempotencyKey) => apiRequest<OnboardingState>('/v1/me/onboarding', { method: 'PUT', body: JSON.stringify(state), idempotencyKey }),
  getPlatforms: () => apiRequest<PlatformCatalog>('/v1/platforms'),
  getEntitlement: () => apiRequest<Entitlement>('/v1/me/entitlement'),
  listArchiveImports: () => apiRequest<ArchiveImportList>('/v1/archive-imports'),
  reserveArchiveImport: (input, idempotencyKey) => apiRequest<ReserveArchiveResponse>('/v1/archive-imports', { method: 'POST', body: JSON.stringify(input), idempotencyKey }),
  completeArchiveImport: (id, idempotencyKey) => apiRequest<ArchiveImport>(`/v1/archive-imports/${encodeURIComponent(id)}/complete`, { method: 'POST', idempotencyKey }),
  getArchiveImport: (id) => apiRequest<ArchiveImport>(`/v1/archive-imports/${encodeURIComponent(id)}`),
  createScan: (archiveImportIds, idempotencyKey) => apiRequest<ScanStatus>('/v1/scans', { method: 'POST', body: JSON.stringify({ archiveImportIds }), idempotencyKey }),
  getCurrentScan: () => apiRequest<ScanStatus>('/v1/scans/current'),
  getScan: (id) => apiRequest<ScanStatus>(`/v1/scans/${encodeURIComponent(id)}`),
  getDashboard: (scanId) => apiRequest<DashboardData>(`/v1/dashboard${query({ scanId })}`),
  getReviewList: (values: ReviewListQuery = {}) => apiRequest<ReviewListData>(`/v1/flags${query(values)}`),
  getFlaggedPost: (id) => apiRequest<FlaggedPost>(`/v1/flags/${encodeURIComponent(id)}`),
  applyReviewAction: (id, action, expectedStatus, idempotencyKey) => apiRequest<FlaggedPost>(`/v1/flags/${encodeURIComponent(id)}/review-actions`, { method: 'POST', body: JSON.stringify({ action, expectedStatus }), idempotencyKey }),
  deleteAccount: (idempotencyKey) => apiRequest<{ purgeDeadline?: string }>('/v1/me', { method: 'DELETE', idempotencyKey }),
};
