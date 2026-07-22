import { ChevronRight } from 'lucide-react-native';
import { Pressable, StyleSheet, View } from 'react-native';

import { RiskBadge, TagPill } from '@/components/primitives/Badges';
import { AppText } from '@/components/primitives/AppText';
import { SurfaceCard } from '@/components/primitives/SurfaceCard';
import { PlatformIcon } from '@/components/PlatformIcon';
import { colors } from '@/theme';
import type { FlaggedPost } from '@/domain/types';

type CardProps = {
  post: FlaggedPost;
  onPress?: () => void;
};

export function FlaggedPostCard({ post, onPress }: CardProps) {
  return (
    <Pressable onPress={onPress} accessibilityRole="button">
      <SurfaceCard elevated={false} style={styles.card}>
        <View style={styles.header}>
          <PlatformIcon platform={post.platform} size={22} />
          <AppText variant="caption" color={colors.fg} weight="600">
            {post.platformLabel}
          </AppText>
          <AppText variant="label" color={colors.muted} weight="400">
            {post.date}
          </AppText>
          <View style={styles.spacer} />
          <RiskBadge level={post.risk} />
        </View>
        <AppText variant="body" color={colors.fg} weight="600">
          {post.quote}
        </AppText>
        <View style={styles.tags}>
          {post.tags.map((tag) => (
            <TagPill key={tag} label={tag} />
          ))}
          <TagPill label={post.engagementLabel} />
          <View style={styles.spacer} />
          <ChevronRight size={16} color={colors.chevronMuted} />
        </View>
      </SurfaceCard>
    </Pressable>
  );
}

export function FlagListRow({ post, onPress }: CardProps) {
  return (
    <Pressable
      accessibilityRole="button"
      onPress={onPress}
      style={styles.row}>
      <PlatformIcon platform={post.platform} size={22} />
      <View style={styles.meta}>
        <AppText variant="caption" color={colors.fg} weight="600" numberOfLines={1}>
          {post.quote}
        </AppText>
        <AppText variant="label" color={colors.muted} weight="400">
          {post.risk.charAt(0).toUpperCase() + post.risk.slice(1)} · {post.category}
        </AppText>
      </View>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  card: {
    gap: 10,
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  spacer: {
    flex: 1,
  },
  tags: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
  },
  row: {
    minHeight: 48,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    paddingVertical: 4,
    borderTopWidth: 1,
    borderTopColor: colors.border,
  },
  meta: {
    flex: 1,
    gap: 1,
  },
});
