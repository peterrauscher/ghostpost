import { BlurView } from 'expo-blur';
import { Lock, Unlock } from 'lucide-react-native';
import { Modal, Platform, StyleSheet, View } from 'react-native';

import { AppText } from '@/components/primitives/AppText';
import { Button } from '@/components/primitives/Button';
import { SurfaceCard } from '@/components/primitives/SurfaceCard';
import { BottomTabBar } from '@/components/Chrome';
import { colors, radii, shadows } from '@/theme';

/**
 * Full-height opacity ramp (top stays optically clear, density builds downward).
 * Slices must be direct flex children — absolute wrappers collapse to 0 height on iOS.
 */
const FROST_SLICES = Array.from({ length: 64 }, (_, i) => {
  const t = i / 63;
  // Keep the top ~25% nearly clear, then ease into heavy frost.
  const shaped = Math.max(0, (t - 0.18) / 0.82);
  return Math.pow(shaped, 1.65) * 0.9;
});

type Props = {
  loading?: boolean;
  onUnlock: () => void;
  onTabPress?: (key: 'home' | 'scan' | 'profile') => void;
};

export function LockedOverlay({ loading = false, onUnlock, onTabPress }: Props) {
  return (
    <Modal transparent visible animationType="none" statusBarTranslucent>
      <View style={styles.modalRoot} pointerEvents="box-none">
        <View style={styles.frostBand} collapsable={false}>
          {Platform.OS !== 'web' ? (
            <BlurView intensity={22} tint="light" style={StyleSheet.absoluteFillObject} />
          ) : null}

          {FROST_SLICES.map((opacity, index) => (
            <View
              key={index}
              collapsable={false}
              pointerEvents="none"
              style={[styles.frostSlice, { backgroundColor: `rgba(191, 163, 232, ${opacity})` }]}
            />
          ))}

          <View style={styles.sheetAnchor} pointerEvents="box-none">
            <SurfaceCard style={[styles.sheet, shadows.sheet]}>
              <View style={styles.lockWell}>
                <Lock size={24} color={colors.accent} />
              </View>
              <AppText variant="headline" align="center">
                scan complete
              </AppText>
              <AppText variant="bodyRegular" color={colors.muted} align="center" style={styles.desc}>
                We flagged posts that match your concerns. Unlock your home feed to review what could
                raise red flags.
              </AppText>
              <Button
                label="reveal my flags"
                icon={Unlock}
                iconPosition="left"
                loading={loading}
                onPress={onUnlock}
              />
            </SurfaceCard>
          </View>
        </View>

        <BottomTabBar active="home" dimmed onPress={onTabPress ?? (() => undefined)} />
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  modalRoot: {
    flex: 1,
  },
  frostBand: {
    flex: 1,
    overflow: 'hidden',
  },
  frostSlice: {
    flex: 1,
  },
  sheetAnchor: {
    ...StyleSheet.absoluteFillObject,
    justifyContent: 'flex-end',
    paddingHorizontal: 16,
    paddingBottom: 28,
  },
  sheet: {
    gap: 8,
    padding: 18,
    borderRadius: radii['3xl'],
    alignItems: 'center',
  },
  lockWell: {
    width: 44,
    height: 44,
    borderRadius: 12,
    backgroundColor: colors.accentSoft,
    borderWidth: 1.5,
    borderColor: colors.accent,
    alignItems: 'center',
    justifyContent: 'center',
    marginBottom: 4,
  },
  desc: {
    lineHeight: 19,
    marginBottom: 8,
  },
});
