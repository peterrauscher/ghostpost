import { router } from 'expo-router';
import { useMemo, useState } from 'react';
import { ActivityIndicator, FlatList, StyleSheet, View } from 'react-native';

import {
  AppText,
  Button,
  FilterChip,
  FlaggedPostCard,
  NavHeader,
  Screen,
} from '@/components';
import { useReviewQuery } from '@/features/hooks';
import { colors } from '@/theme';
import type { RiskLevel } from '@/domain/types';

type FilterKey = 'all' | RiskLevel;

export default function ReviewScreen() {
  const [filter, setFilter] = useState<FilterKey>('all');
  const review = useReviewQuery(filter);
  const data = review.data;

  const filters = useMemo(
    () =>
      [
        { key: 'all' as const, label: 'all', count: data?.filters.all ?? 0 },
        { key: 'high' as const, label: 'high', count: data?.filters.high ?? 0 },
        { key: 'medium' as const, label: 'medium', count: data?.filters.medium ?? 0 },
        { key: 'low' as const, label: 'low', count: data?.filters.low ?? 0 },
      ] as const,
    [data?.filters],
  );

  const posts = data?.posts ?? [];

  return (
    <Screen tone="tint" padded={false} edges={['top', 'bottom']}>
      <View style={styles.header}>
        <NavHeader
          title="review flagged content"
          count={data?.filters.all ?? 0}
          onBack={() => router.back()}
        />
      </View>

      <View style={styles.filters}>
        {filters.map((item) => (
          <FilterChip
            key={item.key}
            label={item.label}
            count={item.count}
            risk={item.key}
            active={filter === item.key}
            onPress={() => setFilter(item.key)}
          />
        ))}
      </View>

      {review.isLoading && !data ? (
        <View style={styles.center}>
          <ActivityIndicator color={colors.accent} />
        </View>
      ) : (
        <FlatList
          data={posts}
          keyExtractor={(item) => item.id}
          contentContainerStyle={styles.list}
          ItemSeparatorComponent={() => <View style={{ height: 10 }} />}
          ListEmptyComponent={
            <AppText color={colors.muted} align="center">
              No flagged posts in this filter.
            </AppText>
          }
          renderItem={({ item }) => (
            <FlaggedPostCard post={item} onPress={() => router.push(`/review/${item.id}`)} />
          )}
        />
      )}

      <View style={styles.footer}>
        <Button
          label={`continue (${posts.length}) →`}
          onPress={() => {
            if (posts[0]) router.push(`/review/${posts[0].id}`);
          }}
          disabled={posts.length === 0}
          variant={posts.length === 0 ? 'disabled' : 'primary'}
        />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  header: {
    paddingHorizontal: 12,
  },
  filters: {
    flexDirection: 'row',
    gap: 8,
    paddingHorizontal: 12,
    paddingBottom: 8,
    paddingTop: 2,
  },
  list: {
    paddingHorizontal: 12,
    paddingBottom: 12,
    flexGrow: 1,
  },
  footer: {
    paddingHorizontal: 16,
    paddingBottom: 12,
    paddingTop: 10,
  },
  center: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
  },
});
