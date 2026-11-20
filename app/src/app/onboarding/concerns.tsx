import { router } from 'expo-router';
import {
  CloudRain,
  Flame,
  Landmark,
  MessageSquareWarning,
  Pill,
  ShieldAlert,
  TriangleAlert,
  Wine,
  type LucideIcon,
} from 'lucide-react-native';
import { useState } from 'react';
import { StyleSheet, View } from 'react-native';
import {
  ChoiceTile,
  errorMessage,
  IconBadge,
  OnboardingFrame,
  OnboardingTitle,
  TileGrid,
  TitleAccent,
  toggleValue,
  type OnboardingStatusView,
} from '@/components/onboarding/OnboardingKit';
import { OnboardingMascot } from '@/components/onboarding/OnboardingMascot';
import { mergeOnboarding } from '@/domain/onboarding';
import { CONCERN_OPTIONS, type ConcernOption } from '@/domain/types';
import { useOnboardingQuery, useSubmitOnboardingMutation } from '@/features/hooks';

const ICONS: Record<ConcernOption, LucideIcon> = {
  inappropriate_language: MessageSquareWarning,
  underage_drinking: Wine,
  drug_use: Pill,
  hate: ShieldAlert,
  political_takes: Landmark,
  controversial_topics: Flame,
  negativity: CloudRain,
  public_image: TriangleAlert,
};

export default function ConcernsScreen() {
  const query = useOnboardingQuery();
  const save = useSubmitOnboardingMutation();
  const [selection, setSelection] = useState<ConcernOption[] | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const onboarding = query.data;
  const selected = selection ?? onboarding?.answers.concerns ?? [];

  const status: OnboardingStatusView | null = onboarding
    ? null
    : query.isError
      ? { kind: 'error', message: errorMessage(query.error, 'Could not load your answers.'), onRetry: () => void query.refetch() }
      : { kind: 'loading', label: 'Loading your answers…' };

  const next = async () => {
    if (!onboarding || save.isPending) return;

    setSaveError(null);

    try {
      await save.mutateAsync(mergeOnboarding(onboarding, { concerns: selected }, 3));
      router.push('/onboarding/platforms');
    } catch (cause: unknown) {
      setSaveError(errorMessage(cause instanceof Error ? cause : null, 'Could not save your answers. Please try again.'));
    }
  };

  return (
    <OnboardingFrame
      step={2}
      title={
        <OnboardingTitle>
          what are you{'\n'}
          <TitleAccent>concerned</TitleAccent> about?
        </OnboardingTitle>
      }
      subtitle="Choose all that apply. We’ll look for these kinds of posts."
      onBack={() => router.dismissTo('/onboarding/coming-up')}
      status={status}
      continueDisabled={!selected.length}
      continueLoading={save.isPending}
      actionError={saveError}
      onContinue={() => void next()}>
      <TileGrid>
        {CONCERN_OPTIONS.map((option) => (
          <ChoiceTile
            key={option.id}
            label={option.label}
            leading={<IconBadge icon={ICONS[option.id]} />}
            selected={selected.includes(option.id)}
            onPress={() => setSelection(toggleValue(selected, option.id))}
          />
        ))}
      </TileGrid>
      <View style={styles.mascot}>
        <OnboardingMascot prop="clipboard" />
      </View>
    </OnboardingFrame>
  );
}

const styles = StyleSheet.create({
  mascot: { marginTop: 20 },
});
