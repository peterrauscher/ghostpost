import { Redirect, router } from 'expo-router';
import { StyleSheet, View } from 'react-native';

import { AppText, Button, OptionPill, Screen, StepProgress } from '@/components';
import { COMING_UP_OPTIONS } from '@/domain/types';
import { useAppState } from '@/providers/app-state';
import { colors } from '@/theme';

export default function ComingUpScreen() {
  const { onboarding, toggleComingUp, setOnboardingStep, gate } = useAppState();

  if (gate !== 'onboarding') {
    return <Redirect href="/" />;
  }

  const selected = onboarding.comingUp;
  const canContinue = selected.length > 0;

  const rows = [
    COMING_UP_OPTIONS.slice(0, 2),
    COMING_UP_OPTIONS.slice(2, 4),
    COMING_UP_OPTIONS.slice(4, 6),
  ];

  return (
    <Screen tone="tint">
      <View style={styles.header}>
        <StepProgress current={1} />
        <View style={styles.titleRow}>
          <AppText variant="title">what&apos;s </AppText>
          <AppText variant="title" color={colors.accent}>
            coming
          </AppText>
          <AppText variant="title"> up?</AppText>
        </View>
      </View>

      <View style={styles.options}>
        {rows.map((row, i) => (
          <View key={i} style={styles.row}>
            {row.map((option) => (
              <View key={option.id} style={styles.pillWrap}>
                <OptionPill
                  label={option.label}
                  selected={selected.includes(option.id)}
                  onPress={() => toggleComingUp(option.id)}
                />
              </View>
            ))}
          </View>
        ))}
      </View>

      <View style={styles.footer}>
        <Button
          label="continue"
          variant={canContinue ? 'primary' : 'disabled'}
          disabled={!canContinue}
          onPress={() => {
            setOnboardingStep(2);
            router.push('/onboarding/concerns');
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
    paddingBottom: 8,
  },
  titleRow: {
    flexDirection: 'row',
    flexWrap: 'wrap',
  },
  options: {
    flex: 1,
    gap: 10,
    paddingTop: 10,
  },
  row: {
    flexDirection: 'row',
    gap: 10,
  },
  pillWrap: {
    flex: 1,
  },
  footer: {
    paddingBottom: 12,
    paddingTop: 10,
  },
});
