import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { queryKeys } from '@/features/query-keys';
import { api } from '@/services/api';
import type { OnboardingAnswers, ReviewAction } from '@/domain/types';

export function useDashboardQuery() {
  return useQuery({
    queryKey: queryKeys.dashboard,
    queryFn: () => api.getDashboard(),
  });
}

export function useReviewQuery(risk: string = 'all') {
  return useQuery({
    queryKey: queryKeys.review(risk),
    queryFn: () => api.getReviewList(risk),
  });
}

export function usePostQuery(id: string) {
  return useQuery({
    queryKey: queryKeys.post(id),
    queryFn: () => api.getFlaggedPost(id),
    enabled: Boolean(id),
  });
}

export function useProfileQuery() {
  return useQuery({
    queryKey: queryKeys.profile,
    queryFn: () => api.getProfile(),
  });
}

export function useSubmitOnboardingMutation() {
  return useMutation({
    mutationFn: (answers: OnboardingAnswers) => api.submitOnboarding(answers),
  });
}

export function useStartScanMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api.startScan(),
    onSuccess: (data) => {
      client.setQueryData(queryKeys.scan, data);
    },
  });
}

export function useScanStatusQuery(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.scan,
    queryFn: () => api.getScanStatus(),
    enabled,
    refetchInterval: (query) =>
      query.state.data?.phase === 'complete' ? false : 450,
  });
}

export function useUnlockHomeMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api.unlockHome(),
    onSuccess: (data) => {
      client.setQueryData(queryKeys.dashboard, data);
    },
  });
}

export function useReviewActionMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, action }: { id: string; action: ReviewAction }) =>
      api.applyReviewAction(id, action),
    onSuccess: (post) => {
      client.setQueryData(queryKeys.post(post.id), post);
      client.invalidateQueries({ queryKey: ['review'] });
      client.invalidateQueries({ queryKey: queryKeys.dashboard });
    },
  });
}

export function useResetDemoMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api.resetDemo(),
    onSuccess: async () => {
      await client.clear();
    },
  });
}
