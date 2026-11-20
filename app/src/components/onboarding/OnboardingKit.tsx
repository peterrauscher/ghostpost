import { LinearGradient } from 'expo-linear-gradient';
import { ArrowRight, Check, ChevronLeft, type LucideIcon } from 'lucide-react-native';
import { Children, Fragment, type ReactNode } from 'react';
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { AppText } from '@/components/primitives/AppText';
import { Button } from '@/components/primitives/Button';
import { colors, layout, shadows } from '@/theme';

export const ONBOARDING_STEPS = 3;

const ICON_BG = '#F1EBFD';
const UNCHECKED = '#C9C3D4';

export function toggleValue<T>(values: readonly T[], value: T): T[] {
  return values.includes(value) ? values.filter((item) => item !== value) : [...values, value];
}

export function errorMessage(error: Error | null, fallback: string): string {
  return error?.message ? error.message : fallback;
}

export function OnboardingTitle({ children }: { children: ReactNode }) {
  return (
    <Text accessibilityRole="header" style={styles.title}>
      {children}
    </Text>
  );
}

export function TitleAccent({ children }: { children: ReactNode }) {
  return <Text style={styles.titleAccent}>{children}</Text>;
}

export type OnboardingStatusView =
  | { kind: 'loading'; label: string }
  | { kind: 'error'; message: string; onRetry(): void };

type FrameProps = {
  step: 1 | 2 | 3;
  title: ReactNode;
  subtitle: string;
  /** Omit when there is no previous onboarding screen. */
  onBack?: () => void;
  /** When set, replaces the body (query loading / failure) instead of rendering options. */
  status?: OnboardingStatusView | null;
  onContinue: () => void;
  continueDisabled?: boolean;
  continueLoading?: boolean;
  actionError?: string | null;
  children?: ReactNode;
};

export function OnboardingFrame({
  step,
  title,
  subtitle,
  onBack,
  status,
  onContinue,
  continueDisabled,
  continueLoading,
  actionError,
  children,
}: FrameProps) {
  const insets = useSafeAreaInsets();
  const blocked = status != null;

  return (
    <LinearGradient colors={['#FBF9FF', '#F5F0FF', '#F1EAFE']} style={styles.root}>
      <View style={[styles.root, { paddingTop: insets.top + 8 }]}>
        <View style={styles.column}>
          <View style={styles.header}>
            {onBack ? (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Back"
                hitSlop={4}
                onPress={onBack}
                style={styles.back}>
                <ChevronLeft size={24} color="#6B6578" strokeWidth={2.2} />
              </Pressable>
            ) : (
              <View style={styles.backSpacer} />
            )}
            <View style={styles.progress} accessibilityLabel={`Step ${step} of ${ONBOARDING_STEPS}`}>
              <Text style={styles.progressCount}>
                {step}
                <Text style={styles.progressTotal}> / {ONBOARDING_STEPS}</Text>
              </Text>
              <View style={styles.track}>
                <View style={[styles.fill, { width: `${(step / ONBOARDING_STEPS) * 100}%` }]} />
              </View>
            </View>
            <View style={styles.backSpacer} />
          </View>

          <ScrollView
            style={styles.scroll}
            contentContainerStyle={styles.scrollContent}
            showsVerticalScrollIndicator={false}
            keyboardShouldPersistTaps="handled">
            {title}
            <AppText variant="bodyLg" color={colors.muted} weight="500" style={styles.subtitle}>
              {subtitle}
            </AppText>
            {status?.kind === 'loading' ? (
              <View style={styles.statusBox} accessibilityLiveRegion="polite">
                <ActivityIndicator color={colors.accent} />
                <AppText variant="bodyRegular" color={colors.muted}>
                  {status.label}
                </AppText>
              </View>
            ) : null}
            {status?.kind === 'error' ? (
              <View style={styles.statusBox} accessibilityLiveRegion="polite">
                <AppText variant="body" color={colors.riskHighText} align="center">
                  {status.message}
                </AppText>
                <Button label="try again" onPress={status.onRetry} style={styles.retry} />
              </View>
            ) : null}
            {blocked ? null : children}
          </ScrollView>

          <View style={[styles.footer, { paddingBottom: Math.max(insets.bottom, 12) + 4 }]}>
            {actionError ? (
              <View style={styles.errorBox} accessibilityLiveRegion="assertive">
                <AppText variant="caption" color={colors.riskHighText} weight="600">
                  {actionError}
                </AppText>
              </View>
            ) : null}
            <Button
              label="continue"
              accessibilityLabel="continue"
              icon={ArrowRight}
              disabled={blocked || continueDisabled}
              loading={continueLoading}
              onPress={onContinue}
              style={styles.continue}
            />
          </View>
        </View>
      </View>
    </LinearGradient>
  );
}

function CheckBadge({ selected, disabled }: { selected: boolean; disabled?: boolean }) {
  if (disabled) return <View style={styles.checkSlot} />;

  return (
    <View style={[styles.checkSlot, selected ? styles.checkOn : styles.checkOff]}>
      {selected ? <Check size={16} color={colors.white} strokeWidth={3} /> : null}
    </View>
  );
}

export function IconBadge({ icon: Icon }: { icon: LucideIcon }) {
  return (
    <View style={styles.iconBadge}>
      <Icon size={24} color={colors.accent} strokeWidth={1.8} />
    </View>
  );
}

type ChoiceProps = {
  /** Canonical label; doubles as the accessible name. */
  label: string;
  selected: boolean;
  onPress?: () => void;
  disabled?: boolean;
};

/** Full-width selectable row (upcoming events). */
export function ChoiceRow({ label, icon, selected, onPress, disabled }: ChoiceProps & { icon: LucideIcon }) {
  return (
    <Pressable
      accessibilityRole="checkbox"
      accessibilityLabel={label}
      aria-checked={selected}
      aria-disabled={disabled}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [styles.card, styles.row, selected && styles.cardOn, pressed && styles.pressed]}>
      <IconBadge icon={icon} />
      <AppText variant="bodyLg" weight="700" style={styles.rowLabel}>
        {label}
      </AppText>
      <CheckBadge selected={selected} />
    </Pressable>
  );
}

/** Half-width selectable tile (concerns, platforms). */
export function ChoiceTile({
  label,
  leading,
  selected,
  onPress,
  disabled,
  note,
}: ChoiceProps & { leading: ReactNode; note?: string }) {
  return (
    <Pressable
      accessibilityRole="checkbox"
      accessibilityLabel={label}
      accessibilityHint={note}
      aria-checked={selected}
      aria-disabled={disabled}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.card,
        styles.tile,
        selected && styles.cardOn,
        disabled && styles.disabled,
        pressed && styles.pressed,
      ]}>
      <View style={styles.tileTop}>
        {leading}
        <CheckBadge selected={selected} disabled={disabled} />
      </View>
      <View>
        <AppText variant="bodyLg" weight="700" style={styles.tileLabel}>
          {label}
        </AppText>
        {note ? (
          <AppText variant="caption" color={colors.muted} weight="600" style={styles.tileNote}>
            {note}
          </AppText>
        ) : null}
      </View>
    </Pressable>
  );
}

/** Lays tiles out two per row; an odd last tile keeps half width. */
export function TileGrid({ children }: { children: ReactNode }) {
  const items = Children.toArray(children);
  const rows: ReactNode[][] = [];

  for (let i = 0; i < items.length; i += 2) rows.push(items.slice(i, i + 2));

  return (
    <View style={styles.grid}>
      {rows.map((row, index) => (
        <View key={index} style={styles.gridRow}>
          {row.map((item, col) => (
            <Fragment key={col}>{item}</Fragment>
          ))}
          {row.length === 1 ? <View style={styles.gridSpacer} /> : null}
        </View>
      ))}
    </View>
  );
}

export function OnboardingCard({ children }: { children: ReactNode }) {
  return <View style={[styles.card, styles.plainCard]}>{children}</View>;
}

const styles = StyleSheet.create({
  root: { flex: 1, width: '100%', alignItems: 'center' },
  column: { flex: 1, width: '100%', maxWidth: layout.maxContentWidth },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingHorizontal: layout.screenPadding,
    minHeight: 48,
  },
  back: {
    width: 44,
    height: 44,
    borderRadius: 16,
    backgroundColor: '#EFE9FB',
    alignItems: 'center',
    justifyContent: 'center',
  },
  backSpacer: { width: 44, height: 44 },
  progress: { flex: 1, alignItems: 'center', gap: 6 },
  progressCount: { fontFamily: 'SN Pro', fontSize: 17, fontWeight: '800', color: colors.accent },
  progressTotal: { fontWeight: '600', color: colors.carouselInactive },
  track: { alignSelf: 'stretch', height: 7, borderRadius: 999, backgroundColor: colors.progressTrack, overflow: 'hidden' },
  fill: { height: '100%', borderRadius: 999, backgroundColor: colors.accent },
  scroll: { flex: 1 },
  scrollContent: { flexGrow: 1, paddingHorizontal: layout.screenPadding, paddingTop: 20, paddingBottom: 20 },
  title: {
    fontFamily: 'SN Pro',
    fontSize: 36,
    lineHeight: 42,
    fontWeight: '800',
    letterSpacing: -1,
    color: colors.black,
  },
  titleAccent: { color: colors.accentDeep },
  subtitle: { marginTop: 12, marginBottom: 22, fontSize: 17, lineHeight: 25 },
  statusBox: { alignItems: 'center', gap: 14, paddingVertical: 40 },
  retry: { minWidth: 160 },
  footer: { paddingHorizontal: layout.screenPadding, paddingTop: 10, gap: 10 },
  continue: { alignSelf: 'stretch' },
  errorBox: { borderRadius: 14, backgroundColor: colors.riskHighBg, paddingHorizontal: 14, paddingVertical: 10 },
  card: {
    backgroundColor: colors.white,
    borderRadius: 22,
    borderWidth: 1.5,
    borderColor: 'transparent',
    ...shadows.card,
  },
  cardOn: { borderColor: colors.accent },
  pressed: { opacity: 0.9 },
  disabled: { opacity: 0.55 },
  plainCard: { padding: 14 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 12, minHeight: 60, paddingHorizontal: 14, paddingVertical: 8 },
  rowLabel: { flex: 1, fontSize: 17, lineHeight: 22 },
  iconBadge: { width: 40, height: 40, borderRadius: 13, backgroundColor: ICON_BG, alignItems: 'center', justifyContent: 'center' },
  checkSlot: { width: 28, height: 28, borderRadius: 14, alignItems: 'center', justifyContent: 'center' },
  checkOn: { backgroundColor: colors.accent },
  checkOff: { borderWidth: 1.5, borderColor: UNCHECKED },
  tile: { flex: 1, minHeight: 104, padding: 12, justifyContent: 'space-between', gap: 10 },
  tileTop: { flexDirection: 'row', alignItems: 'flex-start', justifyContent: 'space-between' },
  tileLabel: { fontSize: 16, lineHeight: 20 },
  tileNote: { marginTop: 2 },
  grid: { gap: 12 },
  gridRow: { flexDirection: 'row', gap: 12 },
  gridSpacer: { flex: 1 },
});
