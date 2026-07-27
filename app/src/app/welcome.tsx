import { Image } from 'expo-image';
import { router } from 'expo-router';
import { useRef, useState } from 'react';
import {
  Dimensions,
  NativeScrollEvent,
  NativeSyntheticEvent,
  Pressable,
  ScrollView,
  StyleSheet,
  View,
} from 'react-native';

import { Button, CarouselDots, Screen, AppText } from '@/components';
import { useAuth } from '@/providers/auth-provider';
import { colors } from '@/theme';

const { width: windowWidth } = Dimensions.get('window');
const slideWidth = Math.min(windowWidth, 480);

const styles = StyleSheet.create({
  pager: {
    flex: 1,
  },
  slide: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 26,
    gap: 8,
  },
  orbit: {
    width: 308,
    height: 264,
    alignItems: 'center',
    justifyContent: 'center',
  },
  mascot: {
    width: 240,
    height: 240,
  },
  orbitIcon: {
    position: 'absolute',
  },
  ig: { width: 44, height: 44, left: -4, top: 6, transform: [{ rotate: '21deg' }] },
  tt: { width: 44, height: 44, right: -6, top: 0, transform: [{ rotate: '-17deg' }] },
  x: { width: 44, height: 44, right: -10, bottom: 14, transform: [{ rotate: '9deg' }] },
  fb: { width: 44, height: 44, left: 2, bottom: 8, transform: [{ rotate: '-13deg' }] },
  reddit: { width: 44, height: 44, left: -14, top: 122, transform: [{ rotate: '5deg' }] },
  connectArt: { width: 160, height: 160 },
  wipeArt: { width: 200, height: 168 },
  headline: { alignItems: 'center' },
  eyebrow: { letterSpacing: 0.6, marginTop: 4 },
  body: { maxWidth: 268, lineHeight: 19 },
  footer: {
    paddingHorizontal: 28,
    paddingBottom: 22,
    paddingTop: 4,
    gap: 12,
  },
  loginRow: {
    flexDirection: 'row',
    justifyContent: 'center',
    gap: 3,
  },
});

const SLIDE_COPY = [
  {
    key: 'brand',
    eyebrow: null as string | null,
    titleLines: [
      { text: "let's clean", color: colors.fg },
      { text: 'your slate', color: colors.accentDeep },
    ] as const,
    body: "we'll scan your social media and help find and remove posts that could hold you back.",
  },
  {
    key: 'connect',
    eyebrow: 'connect & scan',
    titleLines: [
      { text: 'connect your accounts,', color: colors.fg },
      { text: 'we\'ll scan for you.', color: colors.accentDeep },
    ] as const,
    body: 'upload supported platform exports privately. we help you review what admissions, jobs, and rush might notice.',
  },
  {
    key: 'review',
    eyebrow: 'review & wipe',
    titleLines: [
      { text: 'keep what you love,', color: colors.fg },
      { text: 'and clear the rest.', color: colors.accentDeep },
    ] as const,
    body: 'review suggestions privately. actions change your Ghostpost copy only, never your live platform posts.',
  },
] as const;

function BrandArt() {
  return (
    <View style={styles.orbit}>
      <Image
        source={require('@/assets/images/logo-transparent.png')}
        style={styles.mascot}
        contentFit="contain"
      />
      <Image
        source={require('@/assets/icons/instagram-trim.png')}
        style={[styles.orbitIcon, styles.ig]}
        contentFit="contain"
      />
      <Image
        source={require('@/assets/icons/tiktok-trim.png')}
        style={[styles.orbitIcon, styles.tt]}
        contentFit="contain"
      />
      <Image
        source={require('@/assets/icons/x-trim.png')}
        style={[styles.orbitIcon, styles.x]}
        contentFit="contain"
      />
      <Image
        source={require('@/assets/icons/facebook-trim.png')}
        style={[styles.orbitIcon, styles.fb]}
        contentFit="contain"
      />
      <Image
        source={require('@/assets/icons/reddit-trim.png')}
        style={[styles.orbitIcon, styles.reddit]}
        contentFit="contain"
      />
    </View>
  );
}

function ConnectArt() {
  return (
    <Image
      source={require('@/assets/images/ghosts/connect-ghost-suit.png')}
      style={styles.connectArt}
      contentFit="contain"
    />
  );
}

function ReviewArt() {
  return (
    <Image
      source={require('@/assets/images/ghosts/review-wipe-ghost.png')}
      style={styles.wipeArt}
      contentFit="contain"
    />
  );
}

function SlideArt({ slideKey }: { slideKey: (typeof SLIDE_COPY)[number]['key'] }) {
  if (slideKey === 'brand') return <BrandArt />;
  if (slideKey === 'connect') return <ConnectArt />;
  return <ReviewArt />;
}

export default function WelcomeScreen() {
  const auth = useAuth();
  const [index, setIndex] = useState(0);
  const scrollRef = useRef<ScrollView>(null);

  const onScroll = (e: NativeSyntheticEvent<NativeScrollEvent>) => {
    const next = Math.round(e.nativeEvent.contentOffset.x / slideWidth);
    setIndex(next);
  };

  return (
    <Screen tone="welcome" padded={false} edges={['top', 'bottom']}>
      <ScrollView
        ref={scrollRef}
        horizontal
        pagingEnabled
        showsHorizontalScrollIndicator={false}
        onScroll={onScroll}
        scrollEventThrottle={16}
        style={styles.pager}>
        {SLIDE_COPY.map((slide) => (
          <View key={slide.key} style={[styles.slide, { width: slideWidth }]}>
            <SlideArt slideKey={slide.key} />
            {slide.eyebrow ? (
              <AppText variant="body" color={colors.accent} weight="600" style={styles.eyebrow}>
                {slide.eyebrow}
              </AppText>
            ) : null}
            <View style={styles.headline}>
              {slide.titleLines.map((line) => (
                <AppText
                  key={line.text}
                  variant={'display'}
                  color={line.color}
                  align="center">
                  {line.text}
                </AppText>
              ))}
            </View>
            <AppText
              variant="bodyRegular"
              color={colors.muted}
              align="center"
              style={styles.body}>
              {slide.body}
            </AppText>
          </View>
        ))}
      </ScrollView>

      <View style={styles.footer}>
        <CarouselDots count={SLIDE_COPY.length} index={index} />
        <Button label="get started" variant="dark" onPress={() => void auth.signIn()} />
        <Pressable
          accessibilityRole="link"
          onPress={() => router.push('/login')}
          style={styles.loginRow}>
          <AppText variant="caption" color={colors.accent} weight="500" style={{ fontSize: 13 }}>
            already have an account?
          </AppText>
          <AppText variant="caption" color={colors.accent} weight="700" style={{ fontSize: 13 }}>
            log in
          </AppText>
        </Pressable>
      </View>
    </Screen>
  );
}
