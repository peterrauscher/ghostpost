import { StyleSheet, View } from 'react-native';

import { AppText } from '@/components/primitives/AppText';
import { colors, riskColors, type RiskLevel } from '@/theme';

type Props = {
  level: RiskLevel;
  label?: string;
};

const labels: Record<RiskLevel, string> = {
  high: 'High',
  medium: 'Medium',
  low: 'Low',
};

export function RiskBadge({ level, label }: Props) {
  const palette = riskColors[level];
  return (
    <View style={[styles.badge, { backgroundColor: palette.bg }]}>
      <AppText variant="label" color={palette.text} weight="600">
        {label ?? labels[level]}
      </AppText>
    </View>
  );
}

export function TagPill({ label }: { label: string }) {
  return (
    <View style={styles.tag}>
      <AppText variant="label" color={colors.muted} weight="400">
        {label}
      </AppText>
    </View>
  );
}

export function CountBadge({ count }: { count: number | string }) {
  return (
    <View style={styles.count}>
      <AppText variant="caption" color={colors.accent} weight="600">
        {count}
      </AppText>
    </View>
  );
}

const styles = StyleSheet.create({
  badge: {
    height: 22,
    borderRadius: 11,
    paddingHorizontal: 10,
    alignItems: 'center',
    justifyContent: 'center',
  },
  tag: {
    borderRadius: 10,
    backgroundColor: colors.tagBg,
    paddingHorizontal: 8,
    paddingVertical: 3,
  },
  count: {
    borderRadius: 12,
    backgroundColor: colors.accentSoft,
    paddingHorizontal: 8,
    paddingVertical: 4,
  },
});
