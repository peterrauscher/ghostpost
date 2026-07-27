import { router } from 'expo-router';
import { View } from 'react-native';
import { AppText, Button, Screen, SurfaceCard } from '@/components';
import { mergeOnboarding } from '@/domain/onboarding';
import { useOnboardingQuery, useStartScanMutation, useSubmitOnboardingMutation } from '@/features/hooks';
import { useLifecycle } from '@/providers/lifecycle-provider';
import { colors } from '@/theme';

export default function HowItHelpsScreen() {
  const onboarding = useOnboardingQuery(); const save = useSubmitOnboardingMutation(); const scan = useStartScanMutation(); const lifecycle = useLifecycle();
  if (!onboarding.data) return null;
  const proceed = async () => { await save.mutateAsync(mergeOnboarding(onboarding.data, {}, 4, true)); await lifecycle.refetch(); if (lifecycle.readyImportIds.length) { const created = await scan.mutateAsync(lifecycle.readyImportIds); router.replace({ pathname: '/scan', params: { scanId: created.id } }); } else router.replace('/(tabs)/scan'); };
  return <Screen tone="welcome"><View style={{ flex: 1, justifyContent: 'center', gap: 16 }}><AppText variant="title" align="center">how it helps</AppText><SurfaceCard><AppText variant="bodyRegular" color={colors.muted}>Upload your platform export. Ghostpost analyzes allowlisted text, then gives you private review suggestions. Nothing here edits or deletes your live social posts.</AppText></SurfaceCard></View><Button label={lifecycle.readyImportIds.length ? 'start scan →' : 'upload archives →'} loading={save.isPending || scan.isPending} onPress={() => void proceed()} /></Screen>;
}
