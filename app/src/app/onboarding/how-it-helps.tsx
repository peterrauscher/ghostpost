import { Image } from 'expo-image';
import { LinearGradient } from 'expo-linear-gradient';
import { StyleSheet, View } from 'react-native';

import { AppText, Button, HelpStepRow, Screen, StepProgress } from '@/components';
import { useSubmitOnboardingMutation, useStartScanMutation } from '@/features/hooks';
import { useAppState } from '@/providers/app-state';
import { colors, radii } from '@/theme';

export default function HowItHelpsScreen() {
  const { onboarding, completeOnboarding } = useAppState();
  const submit = useSubmitOnboardingMutation();
  const startScan = useStartScanMutation();

  const onStart = async () => {
    await submit.mutateAsync(onboarding);
    await startScan.mutateAsync();
    completeOnboarding();
  };

  return (
    <Screen tone="tint" scroll>
      <View style={styles.header}>
        <StepProgress current={4} />
        <View style={styles.titleBlock}>
          <View style={styles.titleRow}>
            <AppText variant="title">here&apos;s how </AppText>
            <AppText variant="title" color={colors.accent}>
              ghostpost
            </AppText>
            <AppText variant="title"> helps</AppText>
          </View>
          <AppText variant="bodyRegular" color={colors.muted}>
            A quick look at how we keep you ready for what&apos;s next.
          </AppText>
        </View>
      </View>

      <View style={styles.content}>
        <LinearGradient colors={[...colors.helpHeroGradient]} style={styles.hero}>
          <Image
            source={require('@/assets/images/logo-trim.png')}
            style={styles.heroGhost}
            contentFit="contain"
          />
          <AppText variant="micro" color={colors.muted} align="center">
            IMAGE PLACEHOLDER · GHOST + CHECK FLAG
          </AppText>
        </LinearGradient>

        <View style={styles.list}>
          <HelpStepRow
            icon="search"
            title="Scan"
            description="We analyze your posts, comments, public likes, tagged photos, and more."
          />
          <HelpStepRow
            icon="flag"
            title="Flag"
            description="We flag content that matches your concerns."
          />
          <HelpStepRow
            icon="trash"
            title="Review"
            description="You decide what to keep, edit, or remove — we can make your past disappear."
          />
          <HelpStepRow
            icon="check"
            title="Move forward"
            description="Embrace new opportunities with confidence."
          />
        </View>
      </View>

      <View style={styles.footer}>
        <Button
          label="start scan →"
          loading={submit.isPending || startScan.isPending}
          onPress={onStart}
        />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  header: {
    gap: 14,
    paddingTop: 8,
  },
  titleBlock: {
    gap: 10,
  },
  titleRow: {
    flexDirection: 'row',
    flexWrap: 'wrap',
  },
  content: {
    gap: 14,
    paddingTop: 10,
    paddingBottom: 10,
  },
  hero: {
    height: 125,
    borderRadius: radii['3xl'],
    borderWidth: 1,
    borderColor: '#E5DCF4',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 8,
    gap: 4,
  },
  heroGhost: {
    width: 88,
    height: 95,
  },
  list: {
    gap: 10,
  },
  footer: {
    paddingBottom: 12,
    paddingTop: 10,
  },
});
