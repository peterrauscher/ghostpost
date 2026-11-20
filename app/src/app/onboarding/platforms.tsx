import { useQuery } from '@tanstack/react-query';
import { router } from 'expo-router';
import { useMemo, useState } from 'react';
import { StyleSheet, Text, View } from 'react-native';
import { DisclosureConsent } from '@/components/DisclosureConsent';
import {
  ChoiceTile,
  errorMessage,
  OnboardingCard,
  OnboardingFrame,
  OnboardingTitle,
  TileGrid,
  TitleAccent,
  toggleValue,
  type OnboardingStatusView,
} from '@/components/onboarding/OnboardingKit';
import { OnboardingMascot } from '@/components/onboarding/OnboardingMascot';
import { PlatformIcon } from '@/components/PlatformIcon';
import { mergeOnboarding } from '@/domain/onboarding';
import type { PlatformId } from '@/domain/types';
import { useOnboardingQuery, useSubmitOnboardingMutation } from '@/features/hooks';
import { api } from '@/services/api';

const DISPLAY_LABELS: Partial<Record<PlatformId, string>> = { x: 'X (Twitter)' };

const PLATFORM_ORDER: Record<PlatformId, number> = { instagram: 0, x: 1, tiktok: 2, facebook: 3, reddit: 4 };

/** Presentation-only: LinkedIn is not a supported platform and is never selectable or saved. */
function LinkedInLogo() {
  return (
    <View style={styles.linkedIn}>
      <Text style={styles.linkedInGlyph}>in</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  linkedIn: { width: 44, height: 44, borderRadius: 10, backgroundColor: '#0A66C2', alignItems: 'center', justifyContent: 'center' },
  linkedInGlyph: { color: '#FFFFFF', fontSize: 24, fontWeight: '800', letterSpacing: -1 },
  section: { marginTop: 16 },
});

export default function PlatformsScreen() {
  const onboarding = useOnboardingQuery();
  const catalog = useQuery({ queryKey: ['platform-catalog'], queryFn: () => api.getPlatforms() });
  const save = useSubmitOnboardingMutation();
  const [selection, setSelection] = useState<PlatformId[] | null>(null);
  const [consent, setConsent] = useState<boolean | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  const answers = onboarding.data;

  const platforms = useMemo(
    () => catalog.data?.platforms.slice().sort((a, b) => PLATFORM_ORDER[a.id] - PLATFORM_ORDER[b.id]),
    [catalog.data],
  );

  const revision = catalog.data?.revision;
  const enabled = (platforms ?? []).flatMap((platform) => (platform.archiveEnabled ? [platform.id] : []));
  const selected = (selection ?? answers?.answers.platforms ?? []).filter((id) => enabled.includes(id));
  const stored = answers?.answers.disclosureConsent;
  const accepted = consent ?? (stored?.accepted === true && stored.version === revision);

  const ready = !!answers && !!platforms && !!revision;

  const status: OnboardingStatusView | null = ready
    ? null
    : onboarding.isError || catalog.isError
      ? {
          kind: 'error',
          message: errorMessage(onboarding.error ?? catalog.error, 'Could not load your accounts.'),
          onRetry: () => {
            if (onboarding.isError) void onboarding.refetch();

            if (catalog.isError) void catalog.refetch();
          },
        }
      : { kind: 'loading', label: 'Loading platforms…' };

  const finish = async () => {
    if (!answers || !revision || save.isPending) return;

    setSaveError(null);

    try {
      await save.mutateAsync(
        mergeOnboarding(answers, { platforms: selected, disclosureConsent: { accepted: true, version: revision } }, 3, true),
      );
    } catch (cause: unknown) {
      setSaveError(errorMessage(cause instanceof Error ? cause : null, 'Could not save your accounts. Please try again.'));

      return;
    }

    router.replace('/(tabs)/scan');
  };

  return (
    <OnboardingFrame
      step={3}
      title={
        <OnboardingTitle>
          which <TitleAccent>social accounts</TitleAccent>
          {'\n'}do you have?
        </OnboardingTitle>
      }
      subtitle="Choose the accounts you want to scan. More networks are coming soon."
      onBack={() => router.dismissTo('/onboarding/concerns')}
      status={status}
      continueDisabled={!selected.length || !accepted}
      continueLoading={save.isPending}
      actionError={saveError}
      onContinue={() => void finish()}>
      <TileGrid>
        {(platforms ?? []).map((platform) => {
          const isEnabled = platform.archiveEnabled;

          return (
            <ChoiceTile
              key={platform.id}
              label={DISPLAY_LABELS[platform.id] ?? platform.label}
              leading={<PlatformIcon platform={platform.id} size={44} />}
              selected={isEnabled && selected.includes(platform.id)}
              disabled={!isEnabled}
              note={isEnabled ? undefined : 'Coming soon'}
              onPress={() => setSelection(toggleValue(selected, platform.id))}
            />
          );
        })}
        <ChoiceTile key="linkedin" label="LinkedIn" leading={<LinkedInLogo />} selected={false} disabled note="Coming soon" />
      </TileGrid>
      <View style={styles.section}>
        <OnboardingCard>
          <DisclosureConsent accepted={accepted} onChange={setConsent} />
        </OnboardingCard>
      </View>
      <View style={styles.section}>
        <OnboardingMascot prop="phone" />
      </View>
    </OnboardingFrame>
  );
}
