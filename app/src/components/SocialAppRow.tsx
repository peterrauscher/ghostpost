import { Pressable, StyleSheet, View } from 'react-native';

import { Checkbox } from '@/components/primitives/Checkbox';
import { AppText } from '@/components/primitives/AppText';
import { PlatformIcon } from '@/components/PlatformIcon';
import { colors, radii } from '@/theme';
import type { PlatformId } from '@/domain/types';

type Props = {
  platform: PlatformId;
  label: string;
  selected?: boolean;
  onPress?: () => void;
};

export function SocialAppRow({ platform, label, selected = false, onPress }: Props) {
  return (
    <Pressable
      accessibilityRole="checkbox"
      accessibilityState={{ checked: selected }}
      onPress={onPress}
      style={[styles.row, selected ? styles.selected : styles.default]}>
      <PlatformIcon platform={platform} size={28} />
      <AppText variant="body" color={colors.fg} weight="600" style={styles.label}>
        {label}
      </AppText>
      <View style={styles.spacer} />
      <Checkbox checked={selected} onPress={onPress} />
    </Pressable>
  );
}

const styles = StyleSheet.create({
  row: {
    height: 54,
    borderRadius: radii['2xl'],
    paddingHorizontal: 14,
    paddingVertical: 12,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    borderWidth: 1.5,
    width: '100%',
  },
  default: {
    backgroundColor: colors.surface,
    borderColor: colors.border,
  },
  selected: {
    backgroundColor: colors.accentSoft,
    borderColor: colors.accent,
  },
  label: {
    fontSize: 15,
  },
  spacer: {
    flex: 1,
  },
});
