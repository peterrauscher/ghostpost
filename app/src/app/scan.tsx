import { Image } from 'expo-image';
import { useEffect } from 'react';
import { StyleSheet, View } from 'react-native';

import { AppText, Screen } from '@/components';
import { useScanStatusQuery } from '@/features/hooks';
import { useAppState } from '@/providers/app-state';
import { colors, radii } from '@/theme';

export default function ScanScreen() {
  const { completeScan } = useAppState();
  const scan = useScanStatusQuery(true);
  const phase = scan.data?.phase ?? 'connecting';
  const progress = scan.data?.progress ?? 0.1;
  const message = scan.data?.message ?? 'connecting...';

  useEffect(() => {
    if (phase === 'complete') {
      const timer = setTimeout(() => completeScan(), 600);
      return () => clearTimeout(timer);
    }
  }, [phase, completeScan]);

  return (
    <Screen tone="welcome">
      <View style={styles.content}>
        <Image
          source={require('@/assets/images/logo-transparent.png')}
          style={styles.mascot}
          contentFit="contain"
        />
        <View style={styles.statusArea}>
          <AppText variant="headline" color={colors.accentDeep} align="center">
            {message}
          </AppText>
          <View style={styles.track}>
            <View style={[styles.fill, { width: `${Math.max(8, progress * 100)}%` }]} />
          </View>
        </View>
        <View style={styles.phases}>
          {(['connecting', 'scanning', 'flagging'] as const).map((item) => (
            <AppText
              key={item}
              variant="label"
              color={phase === item || (phase === 'complete' && item === 'flagging') ? colors.accent : '#6B6578B3'}
              weight="600">
              {item}
            </AppText>
          ))}
        </View>
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  content: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 24,
    paddingHorizontal: 28,
  },
  mascot: {
    width: 120,
    height: 120,
  },
  statusArea: {
    width: '100%',
    gap: 16,
    alignItems: 'center',
  },
  track: {
    width: 200,
    height: 6,
    borderRadius: radii.full,
    backgroundColor: colors.progressTrack,
    overflow: 'hidden',
  },
  fill: {
    height: 6,
    borderRadius: radii.full,
    backgroundColor: colors.accent,
  },
  phases: {
    flexDirection: 'row',
    gap: 16,
    paddingTop: 22,
  },
});
