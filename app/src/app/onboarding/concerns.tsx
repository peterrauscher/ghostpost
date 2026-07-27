import { router } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';
import { AppText, Button, OptionPill, Screen } from '@/components';
import { CONCERN_OPTIONS, type ConcernOption } from '@/domain/types';
import { mergeOnboarding } from '@/domain/onboarding';
import { useOnboardingQuery, useSubmitOnboardingMutation } from '@/features/hooks';

export default function ConcernsScreen() {
  const query = useOnboardingQuery(); const save = useSubmitOnboardingMutation(); const [selection, setSelection] = useState<ConcernOption[] | null>(null);
  const selected = selection ?? query.data?.answers.concerns ?? [];
  if (!query.data) return null;
  return <Screen tone="welcome" scroll><AppText variant="title">what concerns you?</AppText><View style={{ gap: 10 }}>{CONCERN_OPTIONS.map((option) => <OptionPill key={option.id} label={option.label} selected={selected.includes(option.id)} onPress={() => setSelection((current) => { const values = current ?? selected; return values.includes(option.id) ? values.filter((id) => id !== option.id) : [...values, option.id]; })} />)}</View><Button label="continue →" disabled={!selected.length} loading={save.isPending} onPress={() => void save.mutateAsync(mergeOnboarding(query.data, { concerns: selected }, 3)).then(() => router.push('/onboarding/platforms'))} /></Screen>;
}
