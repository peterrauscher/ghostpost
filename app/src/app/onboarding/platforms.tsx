import { router } from 'expo-router';
import { StyleSheet, View } from 'react-native';

import { AppText, Button, Screen, SocialAppRow, StepProgress } from '@/components';
import { PLATFORM_OPTIONS } from '@/domain/types';
import { useAppState } from '@/providers/app-state';
import { colors } from '@/theme';

export default function PlatformsScreen() {
  const { onboarding, togglePlatform, setOnboardingStep } = useAppState();
  const selected = onboarding.platforms;
  const canContinue = selected.length > 0;

  return (
    <Screen tone="tint">
      <View style={styles.header}>
        <StepProgress current={3} />
        <View style={styles.titleBlock}>
          <View style={styles.titleRow}>
            <AppText variant="title">which </AppText>
            <AppText variant="title" color={colors.accent}>
              social
            </AppText>
            <AppText variant="title"> media</AppText>
          </View>
          <AppText variant="title">apps do you have?</AppText>
          <AppText variant="bodyRegular" color={colors.muted} style={styles.disclaimer}>
            We&apos;ll scan your public posts and comments. Your private data stays private and we
            will never post anything.
          </AppText>
        </View>
      </View>

      <View style={styles.options}>
        {PLATFORM_OPTIONS.map((platform) => (
          <SocialAppRow
            key={platform.id}
            platform={platform.id}
            label={platform.label}
            selected={selected.includes(platform.id)}
            onPress={() => togglePlatform(platform.id)}
          />
        ))}
      </View>

      <View style={styles.footer}>
        <AppText variant="caption" color={colors.muted} align="center">
          You can disconnect at any time.
        </AppText>
        <Button
          label="continue"
          variant={canContinue ? 'primary' : 'disabled'}
          disabled={!canContinue}
          onPress={() => {
            setOnboardingStep(4);
            router.push('/onboarding/how-it-helps');
          }}
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
  disclaimer: {
    lineHeight: 19,
  },
  options: {
    flex: 1,
    gap: 10,
    paddingTop: 10,
  },
  footer: {
    gap: 12,
    paddingBottom: 12,
    paddingTop: 10,
  },
});
