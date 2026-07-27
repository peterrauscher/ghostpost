import { router } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';
import { AppText, Button, OptionPill, Screen } from '@/components';
import { COMING_UP_OPTIONS, type ComingUpOption } from '@/domain/types';
import { mergeOnboarding } from '@/domain/onboarding';
import { useOnboardingQuery, useSubmitOnboardingMutation } from '@/features/hooks';

export default function ComingUpScreen() {
  const query = useOnboardingQuery(); const save = useSubmitOnboardingMutation(); const [selection, setSelection] = useState<ComingUpOption[] | null>(null);
  const selected = selection ?? query.data?.answers.comingUp ?? [];
  if (!query.data) return null;
  return <Screen tone="welcome" scroll><AppText variant="title">what is coming up?</AppText><View style={{ gap: 10 }}>{COMING_UP_OPTIONS.map((option) => <OptionPill key={option.id} label={option.label} selected={selected.includes(option.id)} onPress={() => setSelection((current) => { const values = current ?? selected; return values.includes(option.id) ? values.filter((id) => id !== option.id) : [...values, option.id]; })} />)}</View><Button label="continue →" disabled={!selected.length} loading={save.isPending} onPress={() => void save.mutateAsync(mergeOnboarding(query.data, { comingUp: selected }, 2)).then(() => router.push('/onboarding/concerns'))} /></Screen>;
}
