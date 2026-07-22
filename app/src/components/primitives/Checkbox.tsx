import { Check } from 'lucide-react-native';
import { Pressable, StyleSheet, View } from 'react-native';

import { colors, radii } from '@/theme';

type Props = {
  checked?: boolean;
  onPress?: () => void;
  size?: number;
};

export function Checkbox({ checked = false, onPress, size = 22 }: Props) {
  return (
    <Pressable
      accessibilityRole="checkbox"
      accessibilityState={{ checked }}
      onPress={onPress}
      hitSlop={8}>
      <View
        style={[
          styles.box,
          { width: size, height: size, borderRadius: radii.full },
          checked ? styles.checked : styles.empty,
        ]}>
        {checked ? <Check size={14} color={colors.white} strokeWidth={3} /> : null}
      </View>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  box: {
    alignItems: 'center',
    justifyContent: 'center',
  },
  empty: {
    borderWidth: 1.5,
    borderColor: colors.border,
    backgroundColor: 'transparent',
  },
  checked: {
    backgroundColor: colors.accent,
  },
});
