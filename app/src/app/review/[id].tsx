import { Image } from 'expo-image';
import { router, useLocalSearchParams } from 'expo-router';
import { ActivityIndicator, StyleSheet, View } from 'react-native';

import {
  ActionRow,
  AppText,
  BackButton,
  Button,
  PlatformIcon,
  Screen,
  SurfaceCard,
} from '@/components';
import { usePostQuery, useReviewActionMutation } from '@/features/hooks';
import { colors, riskColors } from '@/theme';
import type { ReviewAction } from '@/domain/types';

export default function FlagDetailScreen() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const postQuery = usePostQuery(id);
  const actionMutation = useReviewActionMutation();
  const post = postQuery.data;

  const apply = async (action: ReviewAction) => {
    if (!id) return;
    await actionMutation.mutateAsync({ id, action });
    if (action === 'resolve' || action === 'delete' || action === 'archive' || action === 'keep') {
      router.back();
    }
  };

  if (postQuery.isLoading && !post) {
    return (
      <Screen tone="detail">
        <View style={styles.center}>
          <ActivityIndicator color={colors.accent} />
        </View>
      </Screen>
    );
  }

  if (!post) {
    return (
      <Screen tone="detail">
        <BackButton onPress={() => router.back()} />
        <View style={styles.center}>
          <AppText>Post not found.</AppText>
        </View>
      </Screen>
    );
  }

  const riskLabel = `${post.risk.charAt(0).toUpperCase() + post.risk.slice(1)} Risk`;

  return (
    <Screen tone="detail" padded={false} scroll edges={['top', 'bottom']}>
      <View style={styles.nav}>
        <BackButton onPress={() => router.back()} />
      </View>

      <View style={styles.content}>
        <View style={styles.riskHeader}>
          <View style={styles.riskCopy}>
            <AppText variant="caption" color={riskColors[post.risk].dot} weight="700">
              {riskLabel}
            </AppText>
            <AppText variant="titleSm">potentially problematic</AppText>
            <AppText variant="bodyRegular" color={colors.muted} style={{ lineHeight: 18 }}>
              {post.explanation}
            </AppText>
          </View>
          <Image
            source={require('@/assets/images/ghosts/worried-ghost-clean.png')}
            style={styles.worried}
            contentFit="contain"
          />
        </View>

        <SurfaceCard style={styles.postCard}>
          <View style={styles.platformRow}>
            <PlatformIcon platform={post.platform} size={22} />
            <AppText variant="caption" color={colors.fg} weight="600">
              {post.platformLabel}
            </AppText>
            <View style={{ flex: 1 }} />
            <AppText variant="label" color={colors.muted} weight="400">
              {post.date}
            </AppText>
          </View>
          <AppText variant="bodyLg" weight="600">
            {post.quote}
          </AppText>
          <View style={styles.stats}>
            <AppText variant="caption" color={colors.muted}>
              ♡ {post.likes}
            </AppText>
            <AppText variant="caption" color={colors.muted}>
              💬 {post.comments}
            </AppText>
          </View>
        </SurfaceCard>

        <View style={styles.section}>
          <AppText variant="caption" color={colors.fg} weight="700" style={{ fontSize: 13 }}>
            Why it’s flagged
          </AppText>
          <AppText variant="bodyRegular" color={colors.muted} style={{ lineHeight: 19 }}>
            {post.whyFlagged}
          </AppText>
        </View>

        <View style={styles.actions}>
          <AppText variant="caption" color={colors.fg} weight="700" style={{ fontSize: 13 }}>
            what you can do
          </AppText>
          <ActionRow
            tone="danger"
            title="Delete this post"
            subtitle={`Remove it from ${post.platformLabel}`}
            onPress={() => apply('delete')}
          />
          <ActionRow
            tone="accent"
            title="Archive instead"
            subtitle="Hide without deleting"
            onPress={() => apply('archive')}
          />
          <ActionRow
            tone="success"
            title="Keep — I’m fine with it"
            subtitle="Mark reviewed, leave live"
            onPress={() => apply('keep')}
          />
        </View>
      </View>

      <View style={styles.footer}>
        <Button
          label="mark as resolved →"
          loading={actionMutation.isPending}
          onPress={() => apply('resolve')}
        />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  nav: {
    paddingHorizontal: 12,
    height: 50,
    justifyContent: 'center',
  },
  content: {
    paddingHorizontal: 18,
    gap: 12,
    paddingBottom: 12,
  },
  riskHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    gap: 8,
    minHeight: 113,
  },
  riskCopy: {
    flex: 1,
    gap: 4,
    maxWidth: 184,
  },
  worried: {
    width: 88,
    height: 88,
  },
  postCard: {
    gap: 8,
  },
  platformRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  stats: {
    flexDirection: 'row',
    gap: 12,
  },
  section: {
    gap: 6,
  },
  actions: {
    gap: 8,
  },
  footer: {
    paddingHorizontal: 18,
    paddingBottom: 12,
    paddingTop: 10,
  },
  center: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
  },
});
