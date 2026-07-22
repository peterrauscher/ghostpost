import { Image } from 'expo-image';
import { StyleSheet, View } from 'react-native';

import { AppText, Button, Screen, SurfaceCard } from '@/components';
import { useStartScanMutation } from '@/features/hooks';
import { useAppState } from '@/providers/app-state';
import { colors } from '@/theme';

/**
 * Functional Scan tab placeholder.
 * Re-running a scan returns the user to the scan progress gate.
 */
export default function ScanTabScreen() {
  const { beginRescan } = useAppState();
  const startScan = useStartScanMutation();

  return (
    <Screen tone="welcome">
      <View style={styles.content}>
        <Image
          source={require('@/assets/images/ghosts/audit-ghost.png')}
          style={styles.art}
          contentFit="contain"
        />
        <AppText variant="title" align="center">
          scan again
        </AppText>
        <AppText variant="bodyRegular" color={colors.muted} align="center">
          This tab is a functional placeholder. Start a new mock scan to revisit the connecting /
          scanning / flagging flow.
        </AppText>
        <SurfaceCard style={styles.card}>
          <AppText variant="caption" color={colors.muted}>
            What happens
          </AppText>
          <AppText variant="bodyRegular">
            We reset unlock state, kick off the mock scan service, and route you through the scan
            screen again.
          </AppText>
        </SurfaceCard>
      </View>
      <View style={styles.footer}>
        <Button
          label="start new scan →"
          loading={startScan.isPending}
          onPress={async () => {
            await startScan.mutateAsync();
            beginRescan();
          }}
        />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  content: {
    flex: 1,
    justifyContent: 'center',
    gap: 14,
    alignItems: 'center',
    paddingHorizontal: 8,
  },
  art: {
    width: 100,
    height: 125,
  },
  card: {
    width: '100%',
    gap: 6,
  },
  footer: {
    gap: 10,
    paddingBottom: 12,
  },
});
