import { router } from 'expo-router';
import { StyleSheet, View } from 'react-native';

import { AppText, Button, OptionPill, Screen, StepProgress } from '@/components';
import { CONCERN_OPTIONS } from '@/domain/types';
import { useAppState } from '@/providers/app-state';
import { colors } from '@/theme';

export default function ConcernsScreen() {
  const { onboarding, toggleConcern, setOnboardingStep } = useAppState();
  const selected = onboarding.concerns;
  const canContinue = selected.length > 0;

  const first = CONCERN_OPTIONS[0];
  const rows = [
    CONCERN_OPTIONS.slice(1, 3),
    CONCERN_OPTIONS.slice(3, 5),
    CONCERN_OPTIONS.slice(5, 7),
  ];

  return (
    <Screen tone="tint">
      <View style={styles.header}>
        <StepProgress current={2} />
        <View>
          <AppText variant="title">anything you&apos;re</AppText>
          <View style={styles.titleRow}>
            <AppText variant="title" color={colors.accent}>
              concerned
            </AppText>
            <AppText variant="title"> about?</AppText>
          </View>
        </View>
      </View>

      <View style={styles.options}>
        <View style={styles.row}>
          <OptionPill
            label={first.label}
            selected={selected.includes(first.id)}
            onPress={() => toggleConcern(first.id)}
          />
        </View>
        {rows.map((row, i) => (
          <View key={i} style={styles.row}>
            {row.map((option) => (
              <View key={option.id} style={styles.pillWrap}>
                <OptionPill
                  label={option.label}
                  selected={selected.includes(option.id)}
                  onPress={() => toggleConcern(option.id)}
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
            setOnboardingStep(3);
            router.push('/onboarding/platforms');
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
    flexWrap: 'wrap',
  },
  pillWrap: {
    flex: 1,
  },
  footer: {
    paddingBottom: 12,
    paddingTop: 10,
  },
});
