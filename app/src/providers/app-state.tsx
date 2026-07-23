import AsyncStorage from '@react-native-async-storage/async-storage';
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

import type {
  AppGate,
  ComingUpOption,
  ConcernOption,
  OnboardingAnswers,
  PlatformId,
} from '@/domain/types';
import { DEFAULT_ONBOARDING } from '@/mocks/fixtures';

const STORAGE_KEY = 'ghostpost-app-state';

export interface AppState {
  hydrated: boolean;
  hasStarted: boolean;
  onboardingComplete: boolean;
  scanComplete: boolean;
  homeUnlocked: boolean;
  onboarding: OnboardingAnswers;
  onboardingStep: number;
}

interface AppStateContextValue extends AppState {
  gate: AppGate;
  setHasStarted: (value: boolean) => void;
  setOnboardingStep: (step: number) => void;
  toggleComingUp: (id: ComingUpOption) => void;
  toggleConcern: (id: ConcernOption) => void;
  togglePlatform: (id: PlatformId) => void;
  completeOnboarding: () => void;
  completeScan: () => void;
  unlockHome: () => void;
  beginRescan: () => void;
  resetApp: () => Promise<void>;
}

const AppStateContext = createContext<AppStateContextValue | null>(null);

const initialState: Omit<AppState, 'hydrated'> = {
  hasStarted: false,
  onboardingComplete: false,
  scanComplete: false,
  homeUnlocked: false,
  onboarding: { ...DEFAULT_ONBOARDING },
  onboardingStep: 1,
};

function deriveGate(state: Omit<AppState, 'hydrated'>): AppGate {
  if (!state.hasStarted) return 'welcome';
  if (!state.onboardingComplete) return 'onboarding';
  if (!state.scanComplete) return 'scan';
  if (!state.homeUnlocked) return 'locked';
  return 'app';
}

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [hydrated, setHydrated] = useState(false);
  const [state, setState] = useState(initialState);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const raw = await AsyncStorage.getItem(STORAGE_KEY);
        if (raw && !cancelled) {
          const parsed = JSON.parse(raw) as Partial<typeof initialState>;
          setState({
            ...initialState,
            ...parsed,
            onboarding: {
              ...DEFAULT_ONBOARDING,
              ...(parsed.onboarding ?? {}),
            },
          });
        }
      } catch {
        // ignore corrupt storage
      } finally {
        if (!cancelled) setHydrated(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!hydrated) return;
    AsyncStorage.setItem(STORAGE_KEY, JSON.stringify(state)).catch(() => undefined);
  }, [hydrated, state]);

  const update = useCallback((patch: Partial<typeof initialState>) => {
    setState((prev) => ({ ...prev, ...patch }));
  }, []);

  const toggleInArray = useCallback(
    <T extends string>(key: keyof OnboardingAnswers, id: T) => {
      setState((prev) => {
        const current = prev.onboarding[key] as T[];
        const next = current.includes(id)
          ? current.filter((item) => item !== id)
          : [...current, id];
        return {
          ...prev,
          onboarding: { ...prev.onboarding, [key]: next },
        };
      });
    },
    [],
  );

  const resetApp = useCallback(async () => {
    setState(initialState);
    await AsyncStorage.removeItem(STORAGE_KEY);
  }, []);

  const value = useMemo<AppStateContextValue>(
    () => ({
      ...state,
      hydrated,
      gate: deriveGate(state),
      setHasStarted: (hasStarted) => update({ hasStarted }),
      setOnboardingStep: (onboardingStep) => update({ onboardingStep }),
      toggleComingUp: (id) => toggleInArray('comingUp', id),
      toggleConcern: (id) => toggleInArray('concerns', id),
      togglePlatform: (id) => toggleInArray('platforms', id),
      completeOnboarding: () =>
        update({ onboardingComplete: true, onboardingStep: 4, scanComplete: false }),
      completeScan: () => update({ scanComplete: true, homeUnlocked: false }),
      unlockHome: () => update({ homeUnlocked: true }),
      beginRescan: () => update({ scanComplete: false, homeUnlocked: false }),
      resetApp,
    }),
    [hydrated, resetApp, state, toggleInArray, update],
  );

  return <AppStateContext.Provider value={value}>{children}</AppStateContext.Provider>;
}

export function useAppState() {
  const ctx = useContext(AppStateContext);
  if (!ctx) throw new Error('useAppState must be used within AppStateProvider');
  return ctx;
}
