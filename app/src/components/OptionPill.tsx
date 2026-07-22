import { Pressable, StyleSheet } from 'react-native';

import { AppText } from '@/components/primitives/AppText';
import { colors, radii } from '@/theme';

type Props = {
  label: string;
  selected?: boolean;
  onPress?: () => void;
};

export function OptionPill({ label, selected = false, onPress }: Props) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected }}
      onPress={onPress}
      style={[styles.pill, selected ? styles.selected : styles.default]}>
      <AppText
        variant="bodyRegular"
        color={selected ? colors.accentDeep : colors.fg}
        weight={selected ? '600' : '500'}
        style={{ fontSize: 14 }}>
        {label}
      </AppText>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  pill: {
    height: 44,
    borderRadius: radii.full,
    paddingHorizontal: 16,
    paddingVertical: 10,
    alignItems: 'center',
    justifyContent: 'center',
    borderWidth: 1.5,
  },
  default: {
    backgroundColor: colors.surface,
    borderColor: colors.border,
  },
  selected: {
    backgroundColor: colors.accentSoft,
    borderColor: colors.accent,
  },
});
