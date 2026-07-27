export const identityKeys = {
  root: (generation: number, userId: string) => ['ghostpost', generation, userId] as const,
  profile: (g: number, u: string) => [...identityKeys.root(g, u), 'me'] as const,
  onboarding: (g: number, u: string) => [...identityKeys.root(g, u), 'onboarding'] as const,
  imports: (g: number, u: string) => [...identityKeys.root(g, u), 'imports'] as const,
  import: (g: number, u: string, id: string) => [...identityKeys.imports(g, u), id] as const,
  scanCurrent: (g: number, u: string) => [...identityKeys.root(g, u), 'scan', 'current'] as const,
  scan: (g: number, u: string, id: string) => [...identityKeys.root(g, u), 'scan', id] as const,
  entitlement: (g: number, u: string) => [...identityKeys.root(g, u), 'entitlement'] as const,
  dashboard: (g: number, u: string, scanId?: string) => [...identityKeys.root(g, u), 'dashboard', scanId ?? 'latest'] as const,
  flags: (g: number, u: string, risk = 'all', scanId?: string) => [...identityKeys.root(g, u), 'flags', scanId ?? 'latest', risk] as const,
  flag: (g: number, u: string, id: string) => [...identityKeys.root(g, u), 'flag', id] as const,
};
