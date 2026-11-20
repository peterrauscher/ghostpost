import { router } from 'expo-router';
import {
  Briefcase,
  Calendar,
  Compass,
  GraduationCap,
  Handshake,
  PartyPopper,
  Rocket,
  UserSearch,
  Users,
  type LucideIcon,
} from 'lucide-react-native';
import { useState } from 'react';
import { StyleSheet, View } from 'react-native';
import {
  ChoiceRow,
  errorMessage,
  OnboardingFrame,
  OnboardingTitle,
  TitleAccent,
  toggleValue,
  type OnboardingStatusView,
} from '@/components/onboarding/OnboardingKit';
import { OnboardingMascot } from '@/components/onboarding/OnboardingMascot';
import { mergeOnboarding } from '@/domain/onboarding';
import { COMING_UP_OPTIONS, type ComingUpOption } from '@/domain/types';
import { useOnboardingQuery, useSubmitOnboardingMutation } from '@/features/hooks';

const ICONS: Record<ComingUpOption, LucideIcon> = {
  job_interviews: Briefcase,
  internships: Rocket,
  college_apps: GraduationCap,
  career_change: Handshake,
  background_check: UserSearch,
  rush: PartyPopper,
  friends_family: Users,
  something_else: Calendar,
  just_concerned: Compass,
};

/** Display order only; ids and labels come from the shared catalog. */
const ORDER: ComingUpOption[] = [
  'job_interviews',
  'internships',
  'college_apps',
  'career_change',
  'background_check',
  'rush',
  'friends_family',
  'something_else',
  'just_concerned',
];

const OPTIONS = [...COMING_UP_OPTIONS].sort((a, b) => ORDER.indexOf(a.id) - ORDER.indexOf(b.id));

export default function ComingUpScreen() {
  const query = useOnboardingQuery();
  const save = useSubmitOnboardingMutation();
  const [selection, setSelection] = useState<ComingUpOption[] | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const onboarding = query.data;
  const selected = selection ?? onboarding?.answers.comingUp ?? [];

  const status: OnboardingStatusView | null = onboarding
    ? null
    : query.isError
      ? { kind: 'error', message: errorMessage(query.error, 'Could not load your answers.'), onRetry: () => void query.refetch() }
      : { kind: 'loading', label: 'Loading your answers…' };

  const next = async () => {
    if (!onboarding || save.isPending) return;

    setSaveError(null);

    try {
      await save.mutateAsync(mergeOnboarding(onboarding, { comingUp: selected }, 2));
      router.push('/onboarding/concerns');
    } catch (cause: unknown) {
      setSaveError(errorMessage(cause instanceof Error ? cause : null, 'Could not save your answers. Please try again.'));
    }
  };

  return (
    <OnboardingFrame
      step={1}
      title={
        <OnboardingTitle>
          what’s <TitleAccent>coming up</TitleAccent>
          {'\n'}for you?
        </OnboardingTitle>
      }
      subtitle="Tell us what’s on the horizon so we can prioritize what matters most."
      status={status}
      continueDisabled={!selected.length}
      continueLoading={save.isPending}
      actionError={saveError}
      onContinue={() => void next()}>
      <View style={styles.list}>
        {OPTIONS.map((option) => (
          <ChoiceRow
            key={option.id}
            label={option.label}
            icon={ICONS[option.id]}
            selected={selected.includes(option.id)}
            onPress={() => setSelection(toggleValue(selected, option.id))}
          />
        ))}
      </View>
      <View style={styles.mascot}>
        <OnboardingMascot prop="calendar" />
      </View>
    </OnboardingFrame>
  );
}

const styles = StyleSheet.create({
  list: { gap: 12 },
  mascot: { marginTop: 20 },
});
