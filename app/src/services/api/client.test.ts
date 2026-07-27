/// <reference types="jest" />

/**
 * Plan 007 transport contracts: 204 handling, RFC7807 problem+json, and
 * single-flight refresh under concurrent 401s.
 *
 * require() + jest.resetModules is intentional: client.ts throws at module init
 * when EXPO_PUBLIC_API_URL is missing, so each case needs an isolated load.
 * (Jest CJS cannot use dynamic import() without --experimental-vm-modules.)
 */

const API_URL = 'https://api.test.ghostpost.local';

type ApiRequest = <T>(
  path: string,
  options?: RequestInit & { idempotencyKey?: string; skipAuthRefresh?: boolean },
) => Promise<T>;

type ProblemErrorInstance = Error & {
  status: number;
  code?: string;
  problem: { title?: string; status?: number; detail?: string };
};

type ProblemErrorCtor = new (problem: {
  type?: string;
  title?: string;
  status?: number;
  detail?: string;
  code?: string;
}) => ProblemErrorInstance;

type ApiErrorCtor = new (status: number, message: string) => Error & { status: number };

type FetchCall = [input?: RequestInfo | URL, init?: RequestInit];

function jsonResponse(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', ...headers },
  });
}

function emptyResponse(status: number, headers: Record<string, string> = {}): Response {
  return new Response(null, { status, headers });
}

function textResponse(status: number, text: string, headers: Record<string, string> = {}): Response {
  return new Response(text, { status, headers });
}

function loadClient(platformOs: 'web' | 'ios' = 'ios'): {
  apiRequest: ApiRequest;
  resetClientSecurityState: () => void;
  ProblemError: ProblemErrorCtor;
  ApiError: ApiErrorCtor;
  setBearerToken: (token: string | null) => Promise<void>;
} {
  jest.resetModules();
  process.env.EXPO_PUBLIC_API_URL = API_URL;

  jest.doMock('react-native', () => ({ Platform: { OS: platformOs } }));
  jest.doMock('expo-secure-store', () => ({
    getItemAsync: jest.fn(async () => null),
    setItemAsync: jest.fn(async () => undefined),
    deleteItemAsync: jest.fn(async () => undefined),
  }));

  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const client = require('./client') as {
    apiRequest: ApiRequest;
    resetClientSecurityState: () => void;
  };
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const problem = require('./problem') as {
    ProblemError: ProblemErrorCtor;
    ApiError: ApiErrorCtor;
  };
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const session = require('./session') as {
    setBearerToken: (token: string | null) => Promise<void>;
  };
  client.resetClientSecurityState();
  return {
    apiRequest: client.apiRequest,
    resetClientSecurityState: client.resetClientSecurityState,
    ProblemError: problem.ProblemError,
    ApiError: problem.ApiError,
    setBearerToken: session.setBearerToken,
  };
}

function callUrl(call: FetchCall): string {
  return String(call[0]);
}

function callMethod(call: FetchCall): string {
  return (call[1]?.method ?? 'GET').toUpperCase();
}

function callAuth(call: FetchCall): string | null {
  return new Headers(call[1]?.headers).get('Authorization');
}

describe('api client transport (Plan 007)', () => {
  const originalFetch = globalThis.fetch;
  const originalApiUrl = process.env.EXPO_PUBLIC_API_URL;

  afterEach(() => {
    globalThis.fetch = originalFetch;
    if (originalApiUrl === undefined) delete process.env.EXPO_PUBLIC_API_URL;
    else process.env.EXPO_PUBLIC_API_URL = originalApiUrl;
    jest.resetModules();
    jest.dontMock('react-native');
    jest.dontMock('expo-secure-store');
  });

  it('returns undefined for 204 No Content without parsing a body', async () => {
    const { apiRequest } = loadClient();
    const fetchMock = jest.fn(async () => emptyResponse(204));
    globalThis.fetch = fetchMock as typeof fetch;

    await expect(apiRequest<void>('/v1/auth/logout', { method: 'POST' })).resolves.toBeUndefined();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const calls = fetchMock.mock.calls as FetchCall[];
    expect(callUrl(calls[0]!)).toBe(`${API_URL}/v1/auth/logout`);
  });

  it('parses application/problem+json into ProblemError with status and detail', async () => {
    const { apiRequest, ProblemError } = loadClient();
    const problemBody = {
      type: 'https://ghostpost.local/problems/conflict',
      title: 'Conflict',
      status: 409,
      detail: 'onboarding revision mismatch',
      code: 'onboarding_revision_conflict',
    };
    globalThis.fetch = jest.fn(async () =>
      jsonResponse(409, problemBody, { 'Content-Type': 'application/problem+json' }),
    ) as typeof fetch;

    let caught: unknown;
    try {
      await apiRequest('/v1/me/onboarding');
    } catch (error) {
      caught = error;
    }

    expect(caught).toBeInstanceOf(ProblemError);
    const problemError = caught as ProblemErrorInstance;
    expect(problemError.name).toBe('ProblemError');
    expect(problemError.status).toBe(409);
    expect(problemError.message).toBe('onboarding revision mismatch');
    expect(problemError.code).toBe('onboarding_revision_conflict');
    expect(problemError.problem.title).toBe('Conflict');
    expect(problemError.problem.status).toBe(409);
  });

  it('falls back to ApiError when the error body is not problem+json', async () => {
    const { apiRequest, ApiError, ProblemError } = loadClient();
    globalThis.fetch = jest.fn(async () => textResponse(500, 'upstream exploded')) as typeof fetch;

    let caught: unknown;
    try {
      await apiRequest('/v1/me');
    } catch (error) {
      caught = error;
    }

    expect(caught).toBeInstanceOf(ApiError);
    expect(caught).not.toBeInstanceOf(ProblemError);
    const apiError = caught as Error & { status: number };
    expect(apiError.name).toBe('ApiError');
    expect(apiError.status).toBe(500);
    expect(apiError.message).toBe('upstream exploded');
  });

  it('issues a single refresh for concurrent 401 responses then retries originals', async () => {
    const { apiRequest, setBearerToken } = loadClient('ios');
    await setBearerToken('stale');

    let refreshCalls = 0;
    let refreshRelease!: (value: Response) => void;
    const refreshGate = new Promise<Response>((resolve) => {
      refreshRelease = resolve;
    });

    const fetchMock = jest.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      const method = (init?.method ?? 'GET').toUpperCase();
      const auth = new Headers(init?.headers).get('Authorization');

      if (url.endsWith('/v1/auth/refresh') && method === 'POST') {
        refreshCalls += 1;
        return refreshGate;
      }

      if (url.endsWith('/v1/dashboard') || url.endsWith('/v1/me')) {
        if (auth === 'Bearer stale') return emptyResponse(401);
        if (auth === 'Bearer fresh') {
          return jsonResponse(200, { ok: true, path: url.endsWith('/v1/me') ? 'me' : 'dashboard' });
        }
        return emptyResponse(401);
      }

      return emptyResponse(404);
    });
    globalThis.fetch = fetchMock as typeof fetch;

    const first = apiRequest<{ ok: boolean; path: string }>('/v1/dashboard');
    const second = apiRequest<{ ok: boolean; path: string }>('/v1/me');

    for (let i = 0; i < 50 && refreshCalls < 1; i += 1) {
      await Promise.resolve();
    }
    expect(refreshCalls).toBe(1);

    refreshRelease(
      jsonResponse(200, {
        session: { kind: 'bearer', token: 'fresh', expiresAt: '2099-01-01T00:00:00Z' },
      }),
    );

    await expect(Promise.all([first, second])).resolves.toEqual([
      { ok: true, path: 'dashboard' },
      { ok: true, path: 'me' },
    ]);

    const calls = fetchMock.mock.calls as FetchCall[];
    const refreshRequests = calls.filter(
      (call) => callUrl(call).endsWith('/v1/auth/refresh') && callMethod(call) === 'POST',
    );
    expect(refreshRequests).toHaveLength(1);

    const retriedWithFresh = calls.filter((call) => {
      const path = callUrl(call);
      return (
        callAuth(call) === 'Bearer fresh' &&
        (path.endsWith('/v1/dashboard') || path.endsWith('/v1/me'))
      );
    });
    expect(retriedWithFresh).toHaveLength(2);
  });
});
