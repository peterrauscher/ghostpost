import { router } from 'expo-router';
import { useState } from 'react';
import { StyleSheet, TextInput, View } from 'react-native';
import { AppText, BackButton, Button, Screen } from '@/components';
import { useAuth } from '@/providers/auth-provider';
import { errorMessage } from '@/components/onboarding/OnboardingKit';
import { colors } from '@/theme';

export default function LoginScreen() {
  const auth = useAuth();
  const [email, setEmail] = useState('');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const emailValid = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim());

  const continueToCode = async () => {
    if (!emailValid || sending) return;
    setSending(true);
    setError(null);
    try {
      await auth.requestCode(email);
      router.push('/login/code');
    } catch (cause: unknown) {
      setError(errorMessage(cause instanceof Error ? cause : null, 'Could not send a sign-in code. Please try again.'));
    } finally {
      setSending(false);
    }
  };

  return (
    <Screen tone="welcome">
      <View style={styles.header}><BackButton onPress={() => router.back()} /></View>
      <View style={styles.content}>
        <AppText variant="title" align="center">
          log in
        </AppText>
        <AppText variant="bodyRegular" color={colors.muted} align="center">
          Enter your email and we'll send a six-digit code. Signing in and signing up work the same way.
        </AppText>
        <TextInput
          accessibilityLabel="Email address"
          autoCapitalize="none"
          autoComplete="email"
          autoCorrect={false}
          inputMode="email"
          keyboardType="email-address"
          onChangeText={setEmail}
          placeholder="you@example.com"
          placeholderTextColor={colors.chevronMuted}
          style={styles.input}
          value={email}
        />
        {error ? <AppText variant="caption" color={colors.accentDeep} align="center">{error}</AppText> : null}
      </View>
      <View style={styles.footer}>
        <Button label="send code" onPress={() => void continueToCode()} disabled={!emailValid} loading={sending} />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  header: { paddingTop: 8 },
  content: { flex: 1, justifyContent: 'center', gap: 14, paddingHorizontal: 8 },
  input: {
    alignSelf: 'stretch',
    backgroundColor: colors.surface,
    borderColor: colors.border,
    borderRadius: 16,
    borderWidth: 1,
    color: colors.fg,
    paddingHorizontal: 16,
    paddingVertical: 14,
    fontSize: 16,
  },
  footer: { paddingBottom: 12 },
});
