import { LinearGradient } from 'expo-linear-gradient';
import { Image } from 'expo-image';
import { Pressable, StyleSheet, View } from 'react-native';

import { AppText } from '@/components/primitives/AppText';
import { colors, radii } from '@/theme';

type GreetingProps = {
  name: string;
  onAvatarPress?: () => void;
};

export function GreetingHeader({ name, onAvatarPress }: GreetingProps) {
  return (
    <View style={styles.greeting}>
      <AppText variant="title" color={colors.fg}>
        hey, {name} 👋
      </AppText>
      <Pressable accessibilityRole="button" onPress={onAvatarPress}>
        <LinearGradient colors={[...colors.avatarGradient]} style={styles.avatar}>
          <Image
            source={require('@/assets/images/ghosts/avatar-ghost.png')}
            style={styles.avatarImage}
            contentFit="contain"
          />
        </LinearGradient>
      </Pressable>
    </View>
  );
}

type AuditProps = {
  headline: string;
  onReview?: () => void;
};

export function AuditBanner({ headline, onReview }: AuditProps) {
  return (
    <LinearGradient colors={[...colors.auditGradient]} style={styles.banner}>
      <AppText variant="caption" color="#FFFFFFEB" weight="600" style={styles.auditLabel}>
        your ghost audit   ⓘ
      </AppText>
      <AppText variant="headline" color={colors.white} style={styles.headline}>
        {headline}
      </AppText>
      <Image
        source={require('@/assets/images/logo-transparent.png')}
        style={styles.bannerGhost}
        contentFit="contain"
      />
      <Pressable
        accessibilityRole="button"
        onPress={onReview}
        style={styles.reviewBtn}>
        <AppText variant="bodyRegular" color={colors.white} weight="600" style={{ fontSize: 14 }}>
          review now →
        </AppText>
      </Pressable>
    </LinearGradient>
  );
}

const styles = StyleSheet.create({
  greeting: {
    height: 46,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 2,
  },
  avatar: {
    width: 40,
    height: 40,
    borderRadius: 20,
    alignItems: 'center',
    justifyContent: 'center',
    overflow: 'hidden',
  },
  avatarImage: {
    width: 28,
    height: 28,
  },
  banner: {
    height: 208,
    borderRadius: radii['3xl'],
    padding: 18,
    overflow: 'hidden',
  },
  auditLabel: {
    marginBottom: 8,
  },
  headline: {
    maxWidth: 150,
    lineHeight: 26,
  },
  bannerGhost: {
    position: 'absolute',
    right: 12,
    top: 28,
    width: 92,
    height: 100,
  },
  reviewBtn: {
    marginTop: 'auto',
    alignSelf: 'flex-start',
    height: 38,
    paddingHorizontal: 18,
    borderRadius: 19,
    backgroundColor: '#292135',
    alignItems: 'center',
    justifyContent: 'center',
  },
});
