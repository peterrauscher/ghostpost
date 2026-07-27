import * as Crypto from 'expo-crypto';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { identityKeys } from '@/features/query-keys';
import { useAuth } from '@/providers/auth-provider';
import { api } from '@/services/api';
import type { OnboardingState } from '@/domain/onboarding';
import type { ReviewAction } from '@/domain/types';

function useIdentity() { const auth = useAuth(); return { generation: auth.sessionGeneration, userId: auth.user?.id ?? '', enabled: auth.status === 'authenticated' && !!auth.user }; }
export function useDashboardQuery(scanId?: string) { const i = useIdentity(); return useQuery({ queryKey: identityKeys.dashboard(i.generation, i.userId, scanId), queryFn: () => api.getDashboard(scanId), enabled: i.enabled }); }
export function useReviewQuery(risk = 'all', scanId?: string) { const i = useIdentity(); return useQuery({ queryKey: identityKeys.flags(i.generation, i.userId, risk, scanId), queryFn: () => api.getReviewList({ risk: risk as 'all'|'high'|'medium'|'low', scanId }), enabled: i.enabled }); }
export function usePostQuery(id: string) { const i = useIdentity(); return useQuery({ queryKey: identityKeys.flag(i.generation, i.userId, id), queryFn: () => api.getFlaggedPost(id), enabled: i.enabled && !!id }); }
export function useProfileQuery() { const i = useIdentity(); return useQuery({ queryKey: identityKeys.profile(i.generation, i.userId), queryFn: () => api.getProfile(), enabled: i.enabled }); }
export function useOnboardingQuery() { const i = useIdentity(); return useQuery({ queryKey: identityKeys.onboarding(i.generation, i.userId), queryFn: () => api.getOnboarding(), enabled: i.enabled }); }
export function useSubmitOnboardingMutation() { const client = useQueryClient(); const i = useIdentity(); return useMutation({ mutationFn: (state: OnboardingState) => api.putOnboarding(state, Crypto.randomUUID()), onSuccess: (data) => client.setQueryData(identityKeys.onboarding(i.generation, i.userId), data) }); }
export function useStartScanMutation() { const client = useQueryClient(); const i = useIdentity(); return useMutation({ mutationFn: (archiveImportIds: string[]) => api.createScan(archiveImportIds, Crypto.randomUUID()), onSuccess: (data) => { client.setQueryData(identityKeys.scan(i.generation, i.userId, data.id), data); client.setQueryData(identityKeys.scanCurrent(i.generation, i.userId), data); } }); }
export function useScanStatusQuery(scanId?: string) { const i = useIdentity(); return useQuery({ queryKey: identityKeys.scan(i.generation, i.userId, scanId ?? 'current'), queryFn: () => scanId ? api.getScan(scanId) : api.getCurrentScan(), enabled: i.enabled, refetchInterval: (query) => query.state.data && ['succeeded','failed','cancelled'].includes(query.state.data.status) ? false : 1000 }); }
export function useReviewActionMutation() { const client = useQueryClient(); const i = useIdentity(); return useMutation({ mutationFn: ({ id, action, expectedStatus = 'open' }: { id: string; action: ReviewAction; expectedStatus?: string }) => api.applyReviewAction(id, action, expectedStatus, Crypto.randomUUID()), onSuccess: (post) => { client.setQueryData(identityKeys.flag(i.generation, i.userId, post.id), post); void client.invalidateQueries({ queryKey: identityKeys.flags(i.generation, i.userId) }); void client.invalidateQueries({ queryKey: identityKeys.dashboard(i.generation, i.userId) }); } }); }
