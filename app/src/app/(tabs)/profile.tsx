import { StyleSheet, View } from 'react-native';

import { AppText, Button, Screen, SurfaceCard } from '@/components';
import { useProfileQuery, useResetDemoMutation } from '@/features/hooks';
import { useAppState } from '@/providers/app-state';
import { getMockOnboardingSnapshot } from '@/services/api/mock';
import { colors } from '@/theme';

/**
 * Functional Profile placeholder for inspecting mock state and resetting the demo.
 */
export default function ProfileScreen() {
  const profile = useProfileQuery();
  const resetDemo = useResetDemoMutation();
  const app = useAppState();
  const snapshot = getMockOnboardingSnapshot();

  return (
    <Screen tone="tint" scroll>
      <View style={styles.content}>
        <AppText variant="title">profile</AppText>
        <AppText variant="bodyRegular" color={colors.muted}>
          Authentication and account settings will land here. For now this screen inspects the mock
          session and lets you reset the demo.
        </AppText>

        <SurfaceCard style={styles.card}>
          <AppText variant="caption" color={colors.muted}>
            User
          </AppText>
          <AppText variant="body" weight="700">
            {profile.data?.name ?? 'Loading…'}
          </AppText>
          <AppText variant="caption" color={colors.muted}>
            id: {profile.data?.id ?? '—'}
          </AppText>
        </SurfaceCard>

        <SurfaceCard style={styles.card}>
          <AppText variant="caption" color={colors.muted}>
            Onboarding snapshot
          </AppText>
          <AppText variant="bodyRegular">Coming up: {app.onboarding.comingUp.join(', ') || '—'}</AppText>
          <AppText variant="bodyRegular">Concerns: {app.onboarding.concerns.join(', ') || '—'}</AppText>
          <AppText variant="bodyRegular">
            Platforms: {app.onboarding.platforms.join(', ') || snapshot.platforms.join(', ') || '—'}
          </AppText>
        </SurfaceCard>

        <SurfaceCard style={styles.card}>
          <AppText variant="caption" color={colors.muted}>
            App gate
          </AppText>
          <AppText variant="body" weight="600">
            {app.gate}
          </AppText>
        </SurfaceCard>
      </View>

      <View style={styles.footer}>
        <Button
          label="reset demo →"
          loading={resetDemo.isPending}
          onPress={async () => {
            await resetDemo.mutateAsync();
            await app.resetApp();
          }}
        />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  content: {
    gap: 14,
    paddingTop: 8,
    flex: 1,
  },
  card: {
    gap: 6,
  },
  footer: {
    paddingBottom: 12,
    paddingTop: 10,
  },
});
