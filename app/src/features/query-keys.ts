export const queryKeys = {
  profile: ['profile'] as const,
  dashboard: ['dashboard'] as const,
  review: (risk = 'all') => ['review', risk] as const,
  post: (id: string) => ['post', id] as const,
  scan: ['scan'] as const,
};
