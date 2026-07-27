import { Image } from 'expo-image';
import { router, useLocalSearchParams } from 'expo-router';
import { useEffect } from 'react';
import { StyleSheet, View } from 'react-native';
import { AppText, Button, Screen } from '@/components';
import { useScanStatusQuery } from '@/features/hooks';
import { useLifecycle } from '@/providers/lifecycle-provider';
import { colors, radii } from '@/theme';

export default function ScanScreen() {
  const params = useLocalSearchParams<{ scanId?: string | string[] }>(); const scanId = Array.isArray(params.scanId) ? params.scanId[0] : params.scanId;
  const scan = useScanStatusQuery(scanId); const lifecycle = useLifecycle(); const data = scan.data;
  useEffect(() => { if (data?.status === 'succeeded') void lifecycle.refetch().then(() => router.replace('/')); }, [data?.status, lifecycle]);
  const terminalFailure = data?.status === 'failed' || data?.status === 'cancelled';
  return <Screen tone="welcome"><View style={styles.content}><Image source={require('@/assets/images/logo-transparent.png')} style={styles.mascot} contentFit="contain" /><AppText variant="headline" color={colors.accentDeep} align="center">{terminalFailure ? 'scan stopped' : data?.message ?? 'connecting…'}</AppText><View style={styles.track}><View style={[styles.fill, { width: `${Math.max(8, (data?.progress ?? .05) * 100)}%` }]} /></View>{terminalFailure ? <><AppText variant="bodyRegular" color={colors.muted} align="center">Your scan did not complete. No results were opened. You can retry from the archive screen.</AppText><Button label="back to archives" onPress={() => router.replace('/(tabs)/scan')} /></> : null}</View></Screen>;
}
const styles = StyleSheet.create({ content: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 24, paddingHorizontal: 28 }, mascot: { width: 120, height: 120 }, track: { width: 200, height: 6, borderRadius: radii.full, backgroundColor: colors.progressTrack, overflow: 'hidden' }, fill: { height: 6, borderRadius: radii.full, backgroundColor: colors.accent } });
