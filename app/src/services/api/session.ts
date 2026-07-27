import * as SecureStore from 'expo-secure-store';
import { Platform } from 'react-native';

const SESSION_KEY = 'ghostpost.session.token';
const PENDING_KEY = 'ghostpost.auth.pending';

export interface PendingAuthFlow { state: string; expiresAt: string; exchangeSecret: string }

let memoryToken: string | null = null;

export async function getBearerToken(): Promise<string | null> {
  if (Platform.OS === 'web') return null;
  memoryToken ??= await SecureStore.getItemAsync(SESSION_KEY);
  return memoryToken;
}

export async function setBearerToken(token: string | null): Promise<void> {
  memoryToken = token;
  if (Platform.OS === 'web') return;
  if (token) await SecureStore.setItemAsync(SESSION_KEY, token);
  else await SecureStore.deleteItemAsync(SESSION_KEY);
}

export async function getPendingAuthFlow(): Promise<PendingAuthFlow | null> {
  if (Platform.OS === 'web') return null;
  const raw = await SecureStore.getItemAsync(PENDING_KEY);
  if (!raw) return null;
  try { return JSON.parse(raw) as PendingAuthFlow; } catch { await SecureStore.deleteItemAsync(PENDING_KEY); return null; }
}

export async function setPendingAuthFlow(flow: PendingAuthFlow | null): Promise<void> {
  if (Platform.OS === 'web') return;
  if (flow) await SecureStore.setItemAsync(PENDING_KEY, JSON.stringify(flow));
  else await SecureStore.deleteItemAsync(PENDING_KEY);
}

export async function clearLocalSession(): Promise<void> {
  await Promise.all([setBearerToken(null), setPendingAuthFlow(null)]);
}
