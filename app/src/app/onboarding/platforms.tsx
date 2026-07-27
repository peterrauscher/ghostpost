import { router } from 'expo-router';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { AppText, Button, Screen, SurfaceCard } from '@/components';
import { DisclosureConsent } from '@/components/DisclosureConsent';
import { mergeOnboarding } from '@/domain/onboarding';
import type { PlatformId } from '@/domain/types';
import { useOnboardingQuery, useSubmitOnboardingMutation } from '@/features/hooks';
import { api } from '@/services/api';
import { colors } from '@/theme';

export default function PlatformsScreen() {
  const onboarding = useOnboardingQuery(); const catalog = useQuery({ queryKey: ['platform-catalog'], queryFn: () => api.getPlatforms() }); const save = useSubmitOnboardingMutation();
  const [selection, setSelection] = useState<PlatformId[] | null>(null); const [accepted, setAccepted] = useState(false);
  const selected = selection ?? onboarding.data?.answers.platforms ?? [];
  if (!onboarding.data) return null;
  return <Screen tone="welcome" scroll><AppText variant="title">choose your platforms</AppText><View style={{ gap: 10 }}>{catalog.data?.platforms.map((platform) => <Pressable key={platform.id} disabled={!platform.archiveEnabled} onPress={() => setSelection((current) => { const values = current ?? selected; return values.includes(platform.id) ? values.filter((id) => id !== platform.id) : [...values, platform.id]; })}><SurfaceCard style={{ opacity: platform.archiveEnabled ? 1 : .5 }}><AppText variant="body" weight="600">{platform.label}</AppText><AppText variant="caption" color={colors.muted}>{platform.archiveEnabled ? (selected.includes(platform.id) ? 'selected' : 'tap to select') : 'Coming soon'}</AppText></SurfaceCard></Pressable>)}</View><DisclosureConsent accepted={accepted} onChange={setAccepted} /><Button label="continue →" disabled={!accepted || !selected.length} loading={save.isPending} onPress={() => void save.mutateAsync(mergeOnboarding(onboarding.data, { platforms: selected, disclosureConsent: { accepted: true, version: catalog.data?.revision ?? onboarding.data.answers.disclosureConsent.version } }, 4)).then(() => router.push('/onboarding/how-it-helps'))} /></Screen>;
}
