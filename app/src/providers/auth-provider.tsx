import * as Crypto from 'expo-crypto';
import * as WebBrowser from 'expo-web-browser';
import { Platform } from 'react-native';
import { useQueryClient } from '@tanstack/react-query';
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { api } from '@/services/api';
import { ApiError } from '@/services/api/problem';
import { clearLocalSession, getBearerToken, getPendingAuthFlow, setBearerToken, setPendingAuthFlow } from '@/services/api/session';
import type { UserProfile } from '@/domain/types';

WebBrowser.maybeCompleteAuthSession();

type AuthStatus = 'loading' | 'authenticated' | 'unauthenticated';
interface AuthContextValue {
  status: AuthStatus; user: UserProfile | null; sessionGeneration: number; error: string | null;
  signIn(): Promise<void>; completeCallback(url: string): Promise<void>; signOut(): Promise<void>;
  localOnlySignOut(): Promise<void>; deleteAccount(): Promise<void>; refreshProfile(): Promise<void>;
}
const AuthContext = createContext<AuthContextValue | null>(null);

function callbackParams(url: string) {
  const parsed = new URL(url);
  if (Platform.OS !== 'web' && (parsed.protocol !== 'ghostpost:' || parsed.hostname !== 'auth' || parsed.pathname !== '/callback')) throw new Error('Invalid authentication callback');
  const codes = parsed.searchParams.getAll('code');
  const states = parsed.searchParams.getAll('state');
  if (codes.length !== 1 || states.length !== 1 || !codes[0] || !states[0]) throw new Error('Authentication callback is incomplete');
  return { code: codes[0], state: states[0] };
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const client = useQueryClient();
  const [status, setStatus] = useState<AuthStatus>('loading');
  const [user, setUser] = useState<UserProfile | null>(null);
  const [sessionGeneration, setGeneration] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const callbackFlight = useRef<Promise<void> | null>(null);

  const refreshProfile = useCallback(async () => {
    try { const profile = await api.getProfile(); setUser(profile); setStatus('authenticated'); }
    catch { setUser(null); setStatus('unauthenticated'); }
  }, []);

  useEffect(() => { void (async () => { if (Platform.OS !== 'web' && !await getBearerToken()) { setStatus('unauthenticated'); return; } await refreshProfile(); })(); }, [refreshProfile]);

  const finishLocalLogout = useCallback(async () => {
    await client.cancelQueries(); client.clear(); await clearLocalSession();
    setUser(null); setStatus('unauthenticated'); setGeneration((value) => value + 1);
  }, [client]);

  const completeCallback = useCallback(async (url: string) => {
    if (callbackFlight.current) return callbackFlight.current;
    callbackFlight.current = (async () => {
      const { code, state } = callbackParams(url);
      try {
        if (Platform.OS === 'web') await api.exchangeWeb(code, state);
        else {
          const pending = await getPendingAuthFlow();
          if (!pending || pending.state !== state || Date.parse(pending.expiresAt) <= Date.now()) { await setPendingAuthFlow(null); throw new Error('Authentication request expired or did not match'); }
          const result = await api.exchangeNative(code, state, pending.exchangeSecret);
          if (result.session.kind !== 'bearer') throw new Error('Unsupported session response');
          await setBearerToken(result.session.token);
          await setPendingAuthFlow(null);
        }
        const profile = await api.getProfile(); setUser(profile); setStatus('authenticated'); setGeneration((value) => value + 1); setError(null);
      } catch (cause) {
        const terminal = cause instanceof ApiError && cause.status >= 400 && cause.status < 500;
        if (terminal) await setPendingAuthFlow(null);
        setError(cause instanceof Error ? cause.message : 'Authentication failed');
        throw cause;
      }
    })().finally(() => { callbackFlight.current = null; });
    return callbackFlight.current;
  }, []);

  const signIn = useCallback(async () => {
    setError(null);
    const kind = Platform.OS === 'web' ? 'web' : 'native';
    const authorization = await api.authorize(kind);
    if (Platform.OS === 'web') { window.location.assign(authorization.authorizationUrl); return; }
    if (!authorization.exchangeSecret) throw new Error('Native authorization did not return exchange proof');
    await setPendingAuthFlow({ state: authorization.state, expiresAt: authorization.expiresAt, exchangeSecret: authorization.exchangeSecret });
    const result = await WebBrowser.openAuthSessionAsync(authorization.authorizationUrl, 'ghostpost://auth/callback');
    if (result.type === 'success') await completeCallback(result.url);
    else { await setPendingAuthFlow(null); setError(result.type === 'cancel' ? 'Sign in was cancelled.' : 'Sign in was dismissed.'); }
  }, [completeCallback]);

  const signOut = useCallback(async () => { await api.logout(); await finishLocalLogout(); }, [finishLocalLogout]);
  const localOnlySignOut = useCallback(async () => { if (Platform.OS === 'web') throw new Error('Web sign-out requires server confirmation'); await finishLocalLogout(); }, [finishLocalLogout]);
  const deleteAccount = useCallback(async () => { await api.deleteAccount(Crypto.randomUUID()); await finishLocalLogout(); }, [finishLocalLogout]);

  const value = useMemo(() => ({ status, user, sessionGeneration, error, signIn, completeCallback, signOut, localOnlySignOut, deleteAccount, refreshProfile }), [status, user, sessionGeneration, error, signIn, completeCallback, signOut, localOnlySignOut, deleteAccount, refreshProfile]);
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() { const value = useContext(AuthContext); if (!value) throw new Error('useAuth must be used inside AuthProvider'); return value; }
