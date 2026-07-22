import { StyleSheet, View, type ViewProps } from 'react-native';

import { colors, radii, shadows } from '@/theme';

type Props = ViewProps & {
  elevated?: boolean;
};

export function SurfaceCard({ elevated = true, style, children, ...props }: Props) {
  return (
    <View style={[styles.card, elevated && shadows.card, style]} {...props}>
      {children}
    </View>
  );
}

const styles = StyleSheet.create({
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii['2xl'],
    borderWidth: 1,
    borderColor: colors.border,
    padding: 14,
  },
});
