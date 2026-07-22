import { Text, type TextProps, type TextStyle } from 'react-native';

import { colors, fonts, typography } from '@/theme';

type TextVariant = keyof typeof typography;

type Props = TextProps & {
  variant?: TextVariant;
  color?: string;
  weight?: TextStyle['fontWeight'];
  align?: TextStyle['textAlign'];
};

export function AppText({
  variant = 'bodyRegular',
  color = colors.fg,
  weight,
  align,
  style,
  ...props
}: Props) {
  const base = typography[variant];
  return (
    <Text
      {...props}
      style={[
        {
          fontFamily: fonts.ui,
          color,
          fontSize: base.fontSize,
          fontWeight: weight ?? base.fontWeight,
          letterSpacing: base.letterSpacing,
          lineHeight: base.lineHeight,
          textAlign: align,
        },
        style,
      ]}
    />
  );
}
