import { ChevronLeft } from 'lucide-react-native';
import { Pressable, StyleSheet, View } from 'react-native';

import { CountBadge } from '@/components/primitives/Badges';
import { AppText } from '@/components/primitives/AppText';
import { colors } from '@/theme';

type BackProps = {
  onPress?: () => void;
};

export function BackButton({ onPress }: BackProps) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel="Go back"
      onPress={onPress}
      style={styles.back}>
      <ChevronLeft size={20} color={colors.fg} />
    </Pressable>
  );
}

type NavProps = {
  title: string;
  count?: number | string;
  onBack?: () => void;
  showCount?: boolean;
};

export function NavHeader({ title, count, onBack, showCount = true }: NavProps) {
  return (
    <View style={styles.nav}>
      <BackButton onPress={onBack} />
      <AppText variant="bodyLg" color={colors.fg} weight="700" align="center" style={styles.title}>
        {title}
      </AppText>
      {showCount && count != null ? <CountBadge count={count} /> : <View style={styles.back} />}
    </View>
  );
}

export function StepProgress({ current, total = 4 }: { current: number; total?: number }) {
  return (
    <View style={styles.step}>
      <AppText variant="captionBold" color={colors.accent}>
        {current}
      </AppText>
      <AppText variant="caption" color={colors.accent} weight="400">
        /{total}
      </AppText>
    </View>
  );
}

export function CarouselDots({ count, index }: { count: number; index: number }) {
  return (
    <View style={styles.dots}>
      {Array.from({ length: count }).map((_, i) => (
        <View
          key={i}
          style={[
            i === index ? styles.dotActive : styles.dotInactive,
          ]}
        />
      ))}
    </View>
  );
}

const styles = StyleSheet.create({
  back: {
    width: 36,
    height: 36,
    borderRadius: 10,
    alignItems: 'center',
    justifyContent: 'center',
  },
  nav: {
    height: 50,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 4,
  },
  title: {
    flex: 1,
  },
  step: {
    flexDirection: 'row',
    alignItems: 'center',
  },
  dots: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    height: 10,
  },
  dotActive: {
    width: 20,
    height: 7,
    borderRadius: 999,
    backgroundColor: colors.accent,
  },
  dotInactive: {
    width: 7,
    height: 7,
    borderRadius: 999,
    backgroundColor: colors.carouselInactive,
  },
});
