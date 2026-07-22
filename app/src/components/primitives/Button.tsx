import { LinearGradient } from 'expo-linear-gradient';
import type { LucideIcon } from 'lucide-react-native';
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  View,
  type PressableProps,
} from 'react-native';

import { AppText } from '@/components/primitives/AppText';
import { colors, radii, shadows } from '@/theme';

type Variant = 'primary' | 'dark' | 'disabled';

type Props = PressableProps & {
  label: string;
  variant?: Variant;
  loading?: boolean;
  icon?: LucideIcon;
  iconPosition?: 'left' | 'right';
};

export function Button({
  label,
  variant = 'primary',
  loading = false,
  disabled,
  icon: Icon,
  iconPosition = 'right',
  style,
  ...props
}: Props) {
  const isDisabled = disabled || variant === 'disabled' || loading;
  const resolvedVariant = isDisabled && variant === 'primary' ? 'disabled' : variant;

  const labelNode = (
    <AppText
      variant={resolvedVariant === 'dark' ? 'bodyLg' : 'body'}
      color={colors.white}
      weight="600">
      {label}
    </AppText>
  );

  const iconNode = Icon ? <Icon size={18} color={colors.white} strokeWidth={2.4} /> : null;

  const content = loading ? (
    <ActivityIndicator color={colors.white} />
  ) : (
    <View style={styles.content}>
      {iconPosition === 'left' ? iconNode : null}
      {labelNode}
      {iconPosition === 'right' ? iconNode : null}
    </View>
  );

  if (resolvedVariant === 'dark') {
    return (
      <Pressable
        accessibilityRole="button"
        disabled={isDisabled}
        style={({ pressed }) => [
          styles.base,
          styles.dark,
          shadows.darkButton,
          pressed && styles.pressed,
          typeof style === 'function' ? style({ pressed, hovered: false }) : style,
        ]}
        {...props}>
        {content}
      </Pressable>
    );
  }

  const gradient =
    resolvedVariant === 'disabled' ? colors.buttonDisabledGradient : colors.buttonGradient;

  return (
    <Pressable
      accessibilityRole="button"
      disabled={isDisabled}
      style={({ pressed }) => [
        isDisabled && styles.disabledWrap,
        pressed && !isDisabled && styles.pressed,
        typeof style === 'function' ? undefined : style,
      ]}
      {...props}>
      <LinearGradient
        colors={[...gradient]}
        start={{ x: 0, y: 0 }}
        end={{ x: 1, y: 1 }}
        style={[
          styles.base,
          resolvedVariant === 'primary' && !isDisabled && shadows.button,
        ]}>
        {content}
      </LinearGradient>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  base: {
    height: 50,
    borderRadius: radii.full,
    alignItems: 'center',
    justifyContent: 'center',
    width: '100%',
  },
  content: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
  },
  dark: {
    height: 54,
    backgroundColor: colors.black,
  },
  disabledWrap: {
    opacity: 0.42,
  },
  pressed: {
    opacity: 0.88,
    transform: [{ scale: 0.99 }],
  },
});
