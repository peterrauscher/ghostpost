import { LinearGradient } from 'expo-linear-gradient';
import {
  ScrollView,
  StyleSheet,
  View,
  type ScrollViewProps,
  type ViewProps,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { colors, layout } from '@/theme';

type Tone = 'tint' | 'welcome' | 'detail' | 'surface';

type ScreenProps = ViewProps & {
  tone?: Tone;
  padded?: boolean;
  scroll?: boolean;
  scrollProps?: ScrollViewProps;
  footer?: React.ReactNode;
  edges?: ('top' | 'bottom')[];
};

const gradients: Record<'welcome' | 'detail', readonly [string, string, ...string[]]> = {
  welcome: colors.welcomeGradient,
  detail: colors.detailGradient,
};

export function Screen({
  tone = 'tint',
  padded = true,
  scroll = false,
  scrollProps,
  footer,
  edges = ['top', 'bottom'],
  style,
  children,
  ...props
}: ScreenProps) {
  const insets = useSafeAreaInsets();
  const paddingStyle = {
    paddingTop: edges.includes('top') ? insets.top : 0,
    paddingBottom: edges.includes('bottom') ? insets.bottom : 0,
  };

  const content = (
    <View
      style={[
        styles.inner,
        padded && styles.padded,
        style,
      ]}
      {...props}>
      {children}
    </View>
  );

  const body = scroll ? (
    <ScrollView
      contentContainerStyle={[styles.scrollContent, padded && styles.padded]}
      showsVerticalScrollIndicator={false}
      {...scrollProps}>
      {children}
    </ScrollView>
  ) : (
    content
  );

  const shell = (
    <View style={[styles.root, paddingStyle, tone === 'tint' && { backgroundColor: colors.screenTint }, tone === 'surface' && { backgroundColor: colors.surface }]}>
      <View style={styles.maxWidth}>
        {body}
        {footer}
      </View>
    </View>
  );

  if (tone === 'welcome' || tone === 'detail') {
    return (
      <LinearGradient colors={[...gradients[tone]]} style={styles.root}>
        <View style={[styles.root, paddingStyle]}>
          <View style={styles.maxWidth}>
            {body}
            {footer}
          </View>
        </View>
      </LinearGradient>
    );
  }

  return shell;
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    width: '100%',
    alignItems: 'center',
  },
  maxWidth: {
    flex: 1,
    width: '100%',
    maxWidth: layout.maxContentWidth,
  },
  inner: {
    flex: 1,
  },
  padded: {
    paddingHorizontal: layout.screenPadding,
  },
  scrollContent: {
    flexGrow: 1,
    paddingBottom: 24,
  },
});
