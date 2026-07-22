import { Image } from 'expo-image';
import { StyleSheet, View } from 'react-native';
import Svg, { Circle } from 'react-native-svg';

import { CountBadge } from '@/components/primitives/Badges';
import { AppText } from '@/components/primitives/AppText';
import { SurfaceCard } from '@/components/primitives/SurfaceCard';
import { colors, riskColors, type RiskLevel } from '@/theme';

type Props = {
  level: RiskLevel;
  flaggedCount: number;
  gaugeSweep?: number;
};

export function RiskSummaryCard({ level, flaggedCount, gaugeSweep = 245 }: Props) {
  const label = level.charAt(0).toUpperCase() + level.slice(1);
  const stroke = riskColors[level].dot;
  const radius = 31;
  const circumference = 2 * Math.PI * radius;
  const progress = Math.min(1, gaugeSweep / 360);
  const dash = circumference * progress;

  return (
    <SurfaceCard style={styles.card}>
      <AppText variant="caption" color={colors.fg} weight="600" style={{ fontSize: 13 }}>
        overall risk level
      </AppText>
      <View style={styles.row}>
        <View style={styles.copy}>
          <AppText
            variant="headline"
            color={riskColors[level].text}
            weight="700"
            style={{ fontSize: 22 }}>
            {label}
          </AppText>
          <AppText variant="caption" color={colors.muted} weight="400">
            {flaggedCount} flagged items
          </AppText>
        </View>
        <View style={styles.gauge}>
          <Svg width={72} height={72}>
            <Circle
              cx={36}
              cy={36}
              r={radius}
              stroke={colors.gaugeTrack}
              strokeWidth={8}
              fill="none"
            />
            <Circle
              cx={36}
              cy={36}
              r={radius}
              stroke={stroke}
              strokeWidth={8}
              fill="none"
              strokeDasharray={`${dash} ${circumference}`}
              strokeLinecap="round"
              rotation={-90}
              origin="36, 36"
            />
          </Svg>
          <Image
            source={require('@/assets/images/ghosts/gauge-ghost.png')}
            style={styles.gaugeGhost}
            contentFit="contain"
          />
        </View>
      </View>
    </SurfaceCard>
  );
}

type FlaggedCardProps = {
  count: number;
  children: React.ReactNode;
};

export function FlaggedPostsCard({ count, children }: FlaggedCardProps) {
  return (
    <SurfaceCard style={styles.flaggedCard}>
      <View style={styles.flaggedHeader}>
        <AppText variant="caption" color={colors.fg} weight="600" style={{ fontSize: 13 }}>
          flagged posts
        </AppText>
        <CountBadge count={count} />
      </View>
      {children}
    </SurfaceCard>
  );
}

const styles = StyleSheet.create({
  card: {
    gap: 8,
    paddingHorizontal: 16,
    paddingVertical: 12,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    height: 72,
  },
  copy: {
    gap: 2,
  },
  gauge: {
    width: 72,
    height: 72,
    alignItems: 'center',
    justifyContent: 'center',
  },
  gaugeGhost: {
    position: 'absolute',
    width: 36,
    height: 36,
  },
  flaggedCard: {
    gap: 6,
    paddingHorizontal: 16,
    paddingVertical: 12,
  },
  flaggedHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
});
