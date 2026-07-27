import { router, useLocalSearchParams } from 'expo-router';
import { useEffect, useState } from 'react';
import { Platform, StyleSheet, View } from 'react-native';
import { AppText, Button, Screen } from '@/components';
import { useAuth } from '@/providers/auth-provider';
import { colors } from '@/theme';

export default function AuthCallbackScreen() {
  const params = useLocalSearchParams<{ code?: string | string[]; state?: string | string[] }>();
  const { completeCallback, status } = useAuth();
  const code = typeof params.code === 'string' ? params.code : null;
  const state = typeof params.state === 'string' ? params.state : null;
  const invalid = !code || !state;
  const [message, setMessage] = useState(invalid ? 'This sign-in link is incomplete.' : 'finishing secure sign in…');

  useEffect(() => {
    if (invalid || !code || !state) return;
    const url = Platform.OS === 'web'
      ? `${window.location.origin}/auth/callback?code=${encodeURIComponent(code)}&state=${encodeURIComponent(state)}`
      : `ghostpost://auth/callback?code=${encodeURIComponent(code)}&state=${encodeURIComponent(state)}`;
    void completeCallback(url).then(() => router.replace('/')).catch((error: unknown) => setMessage(error instanceof Error ? error.message : 'Sign in failed.'));
  }, [completeCallback, invalid, code, state]);

  return <Screen tone="welcome"><View style={styles.content}><AppText variant="title" align="center">signing you in</AppText><AppText variant="bodyRegular" color={colors.muted} align="center">{message}</AppText>{status !== 'authenticated' && message !== 'finishing secure sign in…' ? <Button label="back to login" onPress={() => router.replace('/login')} /> : null}</View></Screen>;
}
const styles = StyleSheet.create({ content: { flex: 1, justifyContent: 'center', gap: 16 } });
