import { ProblemError } from '@/services/api/problem';
import { router } from 'expo-router';
import { useEffect, useRef, useState } from 'react';
import { StyleSheet, TextInput, View } from 'react-native';
import { AppText, BackButton, Button, Screen } from '@/components';
import { useAuth } from '@/providers/auth-provider';
import { colors } from '@/theme';

const RESEND_COOLDOWN_SECS = 30;
const RETRYABLE_CODES: Record<string, true> = { AUTH_CODE_INVALID: true, AUTH_RATE_LIMITED: true };

/** Maps a verify failure to (message, flowAlive). Non-retryable problems send
 * the user back to the email step; everything else stays on this screen. */
function verifyFailure(cause: unknown): { message: string; flowAlive: boolean } {
  if (cause instanceof ProblemError) {
    if (cause.status === 403 || cause.status === 410) {
      return { message: 'Your sign-in request expired. Start again with your email.', flowAlive: false };
    }
    if (RETRYABLE_CODES[cause.code ?? ''] || cause.status === 429) {
      return { message: cause.message, flowAlive: true };
    }
  }
  const message = cause instanceof Error && cause.message ? cause.message : 'Could not verify the code. Please try again.';
  return { message, flowAlive: true };
}

export default function LoginCodeScreen() {
  const auth = useAuth();
  const [code, setCode] = useState('');
  const [verifying, setVerifying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resendIn, setResendIn] = useState(RESEND_COOLDOWN_SECS);
  const inputRef = useRef<TextInput>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (resendIn <= 0) return;
    const timer = setTimeout(() => setResendIn((secs) => secs - 1), 1000);
    return () => clearTimeout(timer);
  }, [resendIn]);

  const email = auth.pendingEmail;

  const verify = async () => {
    if (!/^\d{6}$/.test(code) || verifying) return;
    setVerifying(true);
    setError(null);
    try {
      await auth.verifyCode(code);
      router.replace('/');
    } catch (cause: unknown) {
      const failure = verifyFailure(cause);
      setError(failure.message);
      setCode('');
      if (!failure.flowAlive) router.replace('/login');
    } finally {
      setVerifying(false);
    }
  };

  const resend = async () => {
    if (resendIn > 0 || !email) return;
    setError(null);
    try {
      await auth.requestCode(email);
      setResendIn(RESEND_COOLDOWN_SECS);
      setCode('');
      setError(null);
    } catch (cause: unknown) {
      setError(cause instanceof Error && cause.message ? cause.message : 'Could not resend the code. Please try again.');
    }
  };

  const maskedEmail = email ? email.replace(/^(.).*(@.*)$/, '$1•••••$2') : 'your email';

  return (
    <Screen tone="welcome">
      <View style={styles.header}><BackButton onPress={() => router.back()} /></View>
      <View style={styles.content}>
        <AppText variant="title" align="center">
          check your email
        </AppText>
        <AppText variant="bodyRegular" color={colors.muted} align="center">
          We sent a six-digit code to {maskedEmail}. It expires in ten minutes.
        </AppText>
        <TextInput
          accessibilityLabel="Sign-in code"
          keyboardType="number-pad"
          inputMode="numeric"
          maxLength={6}
          onChangeText={setCode}
          placeholder="••••••"
          placeholderTextColor={colors.chevronMuted}
          ref={inputRef}
          style={styles.input}
          value={code}
        />
        {error ? <AppText variant="caption" color={colors.accentDeep} align="center">{error}</AppText> : null}
        <AppText
          align="center"
          color={resendIn > 0 ? colors.chevronMuted : colors.accentDeep}
          variant="caption"
        >
          {resendIn > 0 ? `resend code in ${resendIn}s` : ''}
        </AppText>
      </View>
      <View style={styles.footer}>
        <Button label="verify and continue" onPress={() => void verify()} disabled={!/^\d{6}$/.test(code)} loading={verifying} />
        {resendIn <= 0 ? <Button label="resend code" onPress={() => void resend()} variant="dark" /> : null}
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
    fontSize: 24,
    letterSpacing: 8,
    paddingHorizontal: 16,
    paddingVertical: 14,
    textAlign: 'center',
  },
  footer: { gap: 10, paddingBottom: 12 },
});
