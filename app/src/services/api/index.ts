import { httpApi } from './http';
import { mockApi, type GhostpostApi } from './mock';

/**
 * Select the API adapter.
 * - Default: delayed in-memory mock (great for UI scaffolding)
 * - Set EXPO_PUBLIC_API_URL to switch to the real HTTP backend
 */
export function createApi(): GhostpostApi {
  if (process.env.EXPO_PUBLIC_API_URL) {
    return httpApi;
  }
  return mockApi;
}

export const api = createApi();

export type { GhostpostApi };
