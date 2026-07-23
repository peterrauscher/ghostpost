import { useRouter } from 'expo-router';
import { ActivityIndicator, StyleSheet, View } from 'react-native';

import {
  AppText,
  AuditBanner,
  FlagListRow,
  FlaggedPostsCard,
  FocusChip,
  GreetingHeader,
  RiskSummaryCard,
  Screen,
} from '@/components';
import { useDashboardQuery } from '@/features/hooks';
import { colors } from '@/theme';

export default function HomeScreen() {
  const router = useRouter();
  const dashboard = useDashboardQuery();
  const data = dashboard.data;

  if (dashboard.isLoading && !data) {
    return (
      <Screen tone="tint">
        <View style={styles.center}>
          <ActivityIndicator color={colors.accent} />
        </View>
      </Screen>
    );
  }

  if (dashboard.isError) {
    return (
      <Screen tone="tint">
        <View style={styles.center}>
          <AppText color={colors.riskHighText}>Couldn&apos;t load dashboard.</AppText>
          <AppText color={colors.accent} onPress={() => dashboard.refetch()}>
            Try again
          </AppText>
        </View>
      </Screen>
    );
  }

  return (
    <Screen tone="tint" padded={false} edges={['top']} scroll>
      <View style={styles.content}>
        <GreetingHeader
          name={data?.user.greetingName ?? 'jordan'}
          onAvatarPress={() => router.navigate('/(tabs)/profile')}
        />
        <AuditBanner
          headline={data?.auditHeadline ?? 'we found posts that could raise red flags'}
          onReview={() => router.navigate('/review')}
        />
        <View style={styles.chips}>
          {(data?.focusAreas ?? []).map((chip) => (
            <FocusChip key={chip.id} label={chip.label} symbol={chip.symbol} />
          ))}
        </View>
        <FlaggedPostsCard count={data?.risk.flaggedCount ?? 0}>
          {(data?.flaggedPreview ?? []).map((post) => (
            <FlagListRow
              key={post.id}
              post={post}
              onPress={() => router.navigate(`/review/${post.id}`)}
            />
          ))}
        </FlaggedPostsCard>
        <RiskSummaryCard
          level={data?.risk.level ?? 'medium'}
          flaggedCount={data?.risk.flaggedCount ?? 0}
          gaugeSweep={data?.risk.gaugeSweep}
        />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  center: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 10,
  },
  content: {
    paddingHorizontal: 16,
    gap: 12,
    paddingBottom: 24,
  },
  chips: {
    flexDirection: 'row',
    gap: 8,
    flexWrap: 'wrap',
  },
});
