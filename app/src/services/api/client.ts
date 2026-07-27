import { Platform } from 'react-native';
import { ApiError, errorFromResponse } from './problem';
import { getBearerToken, setBearerToken } from './session';

const configuredBase = process.env.EXPO_PUBLIC_API_URL;
if (!configuredBase) throw new Error('EXPO_PUBLIC_API_URL is required');
export const API_BASE_URL = configuredBase.replace(/\/$/, '');

let csrfToken: string | null = null;
let refreshFlight: Promise<boolean> | null = null;

function isMutation(method: string) { return !['GET', 'HEAD', 'OPTIONS'].includes(method); }

async function fetchCsrf(): Promise<string> {
  const response = await fetch(`${API_BASE_URL}/v1/auth/csrf`, { credentials: 'include' });
  if (!response.ok) throw await errorFromResponse(response);
  const body = await response.json() as { token?: string; csrfToken?: string };
  const token = body.token ?? body.csrfToken;
  if (!token) throw new ApiError(500, 'CSRF bootstrap returned no token');
  csrfToken = token;
  return token;
}

async function refreshSession(): Promise<boolean> {
  if (!refreshFlight) {
    refreshFlight = (async () => {
      const headers = new Headers();
      const token = await getBearerToken();
      if (token) headers.set('Authorization', `Bearer ${token}`);
      if (Platform.OS === 'web') headers.set('X-CSRF-Token', csrfToken ?? await fetchCsrf());
      const response = await fetch(`${API_BASE_URL}/v1/auth/refresh`, { method: 'POST', headers, credentials: 'include' });
      if (!response.ok) return false;
      if (Platform.OS !== 'web' && response.status !== 204) {
        const body = await response.json() as { session?: { token?: string } };
        if (body.session?.token) await setBearerToken(body.session.token);
      }
      return true;
    })().finally(() => { refreshFlight = null; });
  }
  return refreshFlight;
}

export interface ApiRequestOptions extends RequestInit { idempotencyKey?: string; skipAuthRefresh?: boolean }

export async function apiRequest<T>(path: string, options: ApiRequestOptions = {}): Promise<T> {
  const method = (options.method ?? 'GET').toUpperCase();
  const requiresCsrf = Platform.OS === 'web' && isMutation(method) && path !== '/v1/auth/exchange';
  const execute = async (csrfRetry: boolean): Promise<Response> => {
    const headers = new Headers(options.headers);
    if (options.body != null && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
    const token = await getBearerToken();
    if (token) headers.set('Authorization', `Bearer ${token}`);
    if (options.idempotencyKey) headers.set('Idempotency-Key', options.idempotencyKey);
    if (requiresCsrf) headers.set('X-CSRF-Token', csrfToken ?? await fetchCsrf());
    const response = await fetch(`${API_BASE_URL}${path}`, { ...options, method, headers, credentials: 'include' });
    if (response.status === 403 && requiresCsrf && !csrfRetry) {
      csrfToken = null;
      await fetchCsrf();
      return execute(true);
    }
    return response;
  };

  let response = await execute(false);
  if (response.status === 401 && !options.skipAuthRefresh && path !== '/v1/auth/refresh') {
    if (await refreshSession()) response = await execute(false);
  }
  if (!response.ok) throw await errorFromResponse(response);
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export function resetClientSecurityState() { csrfToken = null; refreshFlight = null; }
