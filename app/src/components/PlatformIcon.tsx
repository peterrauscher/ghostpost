import { Image } from 'expo-image';
import { StyleSheet, View } from 'react-native';

import { AppText } from '@/components/primitives/AppText';
import { colors, radii } from '@/theme';
import type { PlatformId } from '@/domain/types';
import { PLATFORM_OPTIONS } from '@/domain/types';

const images: Partial<Record<PlatformId, number>> = {
  instagram: require('@/assets/icons/instagram-trim.png'),
  tiktok: require('@/assets/icons/tiktok-v2.png'),
  x: require('@/assets/icons/x-trim.png'),
};

type Props = {
  platform: PlatformId;
  size?: number;
  glyphOverride?: string;
};

export function PlatformIcon({ platform, size = 28, glyphOverride }: Props) {
  const meta = PLATFORM_OPTIONS.find((p) => p.id === platform);
  const image = images[platform];

  if (image) {
    return (
      <Image
        source={image}
        style={{ width: size, height: size, borderRadius: radii.sm }}
        contentFit="contain"
      />
    );
  }

  return (
    <View
      style={[
        styles.mark,
        {
          width: size,
          height: size,
          backgroundColor: meta?.color ?? colors.accent,
          borderRadius: size > 24 ? 8 : 6,
        },
      ]}>
      <AppText
        variant="label"
        color={colors.white}
        weight="700"
        style={{ fontSize: size > 24 ? 11 : 9 }}>
        {glyphOverride ?? meta?.glyph ?? '?'}
      </AppText>
    </View>
  );
}

const styles = StyleSheet.create({
  mark: {
    alignItems: 'center',
    justifyContent: 'center',
  },
});
