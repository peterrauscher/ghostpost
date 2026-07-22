import { Pressable, StyleSheet, View } from 'react-native';

import { AppText } from '@/components/primitives/AppText';
import { colors, radii, riskColors, type RiskLevel } from '@/theme';

type FilterProps = {
  label: string;
  count: number;
  active?: boolean;
  risk?: RiskLevel | 'all';
  onPress?: () => void;
};

export function FilterChip({ label, count, active = false, risk = 'all', onPress }: FilterProps) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected: active }}
      onPress={onPress}
      style={[styles.chip, active ? styles.active : styles.inactive]}>
      {risk !== 'all' && !active ? (
        <AppText style={{ color: riskColors[risk].dot, fontSize: 10 }}>●</AppText>
      ) : null}
      <AppText
        variant="caption"
        color={active ? colors.white : colors.muted}
        weight="500"
        style={{ fontSize: 13 }}>
        {label}
      </AppText>
      <AppText
        variant="label"
        color={active ? colors.white : colors.muted}
        weight="500">
        {count}
      </AppText>
    </Pressable>
  );
}

type FocusProps = {
  label: string;
  symbol: string;
};

export function FocusChip({ label, symbol }: FocusProps) {
  return (
    <View style={styles.focus}>
      <View style={styles.focusIcon}>
        <AppText style={{ fontSize: 10 }}>{symbol}</AppText>
      </View>
      <AppText variant="caption" color={colors.muted} weight="500">
        {label}
      </AppText>
    </View>
  );
}

const styles = StyleSheet.create({
  chip: {
    height: 34,
    borderRadius: radii.xl,
    paddingHorizontal: 10,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
    borderWidth: 1,
  },
  active: {
    backgroundColor: colors.accent,
    borderColor: colors.accent,
  },
  inactive: {
    backgroundColor: colors.surface,
    borderColor: colors.border,
  },
  focus: {
    height: 34,
    borderRadius: radii.xl,
    paddingHorizontal: 11,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    backgroundColor: colors.surface,
    borderWidth: 1,
    borderColor: colors.border,
  },
  focusIcon: {
    width: 18,
    height: 18,
    borderRadius: 9,
    backgroundColor: colors.accentSoft,
    alignItems: 'center',
    justifyContent: 'center',
  },
});
