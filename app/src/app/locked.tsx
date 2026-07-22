import {
  AuditBanner,
  FlagListRow,
  FlaggedPostsCard,
  FocusChip,
  GreetingHeader,
  LockedOverlay,
  RiskSummaryCard,
  Screen,
} from '@/components';
import { useDashboardQuery, useUnlockHomeMutation } from '@/features/hooks';
import { useAppState } from '@/providers/app-state';
import { StyleSheet, View } from 'react-native';

export default function LockedHomeScreen() {
  const { unlockHome } = useAppState();
  const dashboard = useDashboardQuery();
  const unlockMutation = useUnlockHomeMutation();
  const data = dashboard.data;

  const onUnlock = async () => {
    await unlockMutation.mutateAsync();
    unlockHome();
  };

  return (
    <>
      <Screen tone="tint" padded={false} edges={['top']}>
        <View style={styles.dashboard}>
          <View style={styles.padded}>
            <GreetingHeader name={data?.user.greetingName ?? 'jordan'} />
            <AuditBanner
              headline={data?.auditHeadline ?? 'we found posts that could raise red flags'}
            />
            <View style={styles.chips}>
              {(data?.focusAreas ?? []).map((chip) => (
                <FocusChip key={chip.id} label={chip.label} symbol={chip.symbol} />
              ))}
            </View>
            <FlaggedPostsCard count={data?.risk.flaggedCount ?? 0}>
              {(data?.flaggedPreview ?? []).map((post) => (
                <FlagListRow key={post.id} post={post} />
              ))}
            </FlaggedPostsCard>
            <RiskSummaryCard
              level={data?.risk.level ?? 'medium'}
              flaggedCount={data?.risk.flaggedCount ?? 0}
              gaugeSweep={data?.risk.gaugeSweep}
            />
          </View>
        </View>
      </Screen>

      <LockedOverlay loading={unlockMutation.isPending} onUnlock={onUnlock} />
    </>
  );
}

const styles = StyleSheet.create({
  dashboard: {
    flex: 1,
  },
  padded: {
    paddingHorizontal: 16,
    gap: 12,
    paddingBottom: 10,
  },
  chips: {
    flexDirection: 'row',
    gap: 8,
    flexWrap: 'wrap',
  },
});
