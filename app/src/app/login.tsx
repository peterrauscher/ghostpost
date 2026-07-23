import { router } from 'expo-router';
import { StyleSheet, View } from 'react-native';

import { AppText, BackButton, Button, Screen, SurfaceCard } from '@/components';
import { useAppState } from '@/providers/app-state';
import { colors } from '@/theme';

/**
 * Functional placeholder for authentication.
 * Real OAuth / session wiring will replace the temporary continue action.
 */
export default function LoginScreen() {
  const { setHasStarted, completeOnboarding, completeScan } = useAppState();

  return (
    <Screen tone="welcome">
      <View style={styles.header}>
        <BackButton onPress={() => router.back()} />
      </View>
      <View style={styles.content}>
        <AppText variant="title" align="center">
          log in
        </AppText>
        <AppText variant="bodyRegular" color={colors.muted} align="center" style={styles.copy}>
          Authentication is scaffolded but not connected yet. Continue as a returning demo user to
          jump to the post-scan paywall.
        </AppText>
        <SurfaceCard style={styles.card}>
          <AppText variant="caption" color={colors.muted}>
            Coming soon
          </AppText>
          <AppText variant="body" weight="600">
            Secure session via Expo SecureStore + OAuth providers
          </AppText>
        </SurfaceCard>
      </View>
      <View style={styles.footer}>
        <Button
          label="continue as demo user →"
          onPress={() => {
            setHasStarted(true);
            completeOnboarding();
            completeScan();
          }}
        />
        <Button
          label="start fresh onboarding"
          variant="dark"
          onPress={() => {
            setHasStarted(true);
          }}
        />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  header: {
    paddingTop: 8,
  },
  content: {
    flex: 1,
    justifyContent: 'center',
    gap: 14,
    paddingHorizontal: 8,
  },
  copy: {
    lineHeight: 19,
  },
  card: {
    gap: 6,
  },
  footer: {
    gap: 10,
    paddingBottom: 12,
  },
});
