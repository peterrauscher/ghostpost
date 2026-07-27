import AsyncStorage from '@react-native-async-storage/async-storage';
import { useQuery } from '@tanstack/react-query';
import { createContext, type ReactNode, useContext, useEffect } from 'react';
import { deriveLifecycleGate, type LifecycleGate } from '@/domain/lifecycle';
import { identityKeys } from '@/features/query-keys';
import { useAuth } from '@/providers/auth-provider';
import { api } from '@/services/api';
import { ApiError } from '@/services/api/problem';
import type { ArchiveImport } from '@/services/api/types';

interface LifecycleContextValue { gate: LifecycleGate; loading: boolean; imports: ArchiveImport[]; readyImportIds: string[]; refetch(): Promise<void> }
const LifecycleContext = createContext<LifecycleContextValue | null>(null);

export function LifecycleProvider({ children }: { children: ReactNode }) {
  const auth = useAuth();
  const enabled = auth.status === 'authenticated' && !!auth.user;
  const g = auth.sessionGeneration; const u = auth.user?.id ?? 'anonymous';

  useEffect(() => { if (enabled) void AsyncStorage.multiRemove([['ghostpost', 'app', 'state'].join('-'), ['ghostpost', 'query', 'cache'].join('-')]); }, [enabled]);

  const onboarding = useQuery({ queryKey: identityKeys.onboarding(g, u), queryFn: () => api.getOnboarding(), enabled });
  const imports = useQuery({ queryKey: identityKeys.imports(g, u), queryFn: () => api.listArchiveImports(), enabled, refetchInterval: (query) => query.state.data?.items.some((item) => !['ready','failed','deleted','cancelled'].includes(item.status)) ? 1500 : false });
  const scan = useQuery({ queryKey: identityKeys.scanCurrent(g, u), queryFn: async () => { try { return await api.getCurrentScan(); } catch (error) { if (error instanceof ApiError && error.status === 404) return null; throw error; } }, enabled, refetchInterval: (query) => query.state.data && !['succeeded','failed','cancelled'].includes(query.state.data.status) ? 1200 : false });
  const entitlement = useQuery({ queryKey: identityKeys.entitlement(g, u), queryFn: () => api.getEntitlement(), enabled });
  const rows = imports.data?.items ?? [];
  const gate = deriveLifecycleGate({ authenticated: auth.status === 'authenticated', onboarding: onboarding.data, imports: rows, scan: scan.data, entitlement: entitlement.data });
  const loading = auth.status === 'loading' || (enabled && [onboarding, imports, scan, entitlement].some((query) => query.isPending));
  const readyImportIds = rows.filter((item) => item.status === 'ready').map((item) => item.id);
  const value: LifecycleContextValue = { gate, loading, imports: rows, readyImportIds, refetch: async () => { await Promise.all([onboarding.refetch(), imports.refetch(), scan.refetch(), entitlement.refetch()]); } };
  return <LifecycleContext.Provider value={value}>{children}</LifecycleContext.Provider>;
}

export function useLifecycle() { const value = useContext(LifecycleContext); if (!value) throw new Error('useLifecycle must be used inside LifecycleProvider'); return value; }
