import { Check, Shield } from 'lucide-react-native';
import { useState } from 'react';
import { ActivityIndicator, Pressable, StyleSheet, View } from 'react-native';

import { AppText, Button, Screen, SurfaceCard } from '@/components';
import { DEFAULT_PLAN_ID, PRICING_PLANS, type PricingPlanId } from '@/domain/pricing';
import { useDashboardQuery, useUnlockHomeMutation } from '@/features/hooks';
import { useAppState } from '@/providers/app-state';
import { colors, radii, shadows } from '@/theme';

export default function LockedHomeScreen() {
  const { unlockHome } = useAppState();
  const dashboard = useDashboardQuery();
  const unlockMutation = useUnlockHomeMutation();
  const [selectedId, setSelectedId] = useState<PricingPlanId>(DEFAULT_PLAN_ID);

  const data = dashboard.data;
  const selected = PRICING_PLANS.find((plan) => plan.id === selectedId) ?? PRICING_PLANS[1];
  const flaggedCount = data?.risk.flaggedCount ?? 7;
  const riskLevel = data?.risk.level ?? 'medium';

  const onPurchase = async () => {
    await unlockMutation.mutateAsync();
    unlockHome();
  };

  if (dashboard.isLoading && !data) {
    return (
      <Screen tone="welcome">
        <View style={styles.center}>
          <ActivityIndicator color={colors.accent} />
        </View>
      </Screen>
    );
  }

  return (
    <Screen
      tone="welcome"
      scroll
      footer={
        <View style={styles.footer}>
          <Button
            label={`Continue · ${selected.priceLabel}`}
            loading={unlockMutation.isPending}
            onPress={onPurchase}
          />
          <AppText variant="caption" color={colors.muted} align="center">
            One-time purchase · no auto-renewal
          </AppText>
        </View>
      }>
      <View style={styles.content}>
        <View style={styles.hero}>
          <View style={styles.shield}>
            <Shield size={22} color={colors.accent} strokeWidth={2.4} />
          </View>
          <AppText variant="micro" color={colors.accent} align="center" style={styles.eyebrow}>
            scan complete
          </AppText>
          <AppText variant="title" align="center">
            unlock your clean slate
          </AppText>
          <AppText variant="bodyRegular" color={colors.muted} align="center" style={styles.lead}>
            We found {flaggedCount} posts that could raise red flags
            {riskLevel ? ` · ${riskLevel} overall risk` : ''}. Choose a pass to review and clean them
            up.
          </AppText>
        </View>

        <View style={styles.plans}>
          {PRICING_PLANS.map((plan) => {
            const selectedPlan = plan.id === selectedId;
            return (
              <Pressable
                key={plan.id}
                accessibilityRole="button"
                accessibilityState={{ selected: selectedPlan }}
                onPress={() => setSelectedId(plan.id)}
                style={({ pressed }) => [pressed && styles.pressed]}>
                <SurfaceCard
                  elevated={selectedPlan}
                  style={[
                    styles.planCard,
                    selectedPlan ? styles.planSelected : styles.planIdle,
                    plan.badge ? shadows.card : null,
                  ]}>
                  <View style={styles.planHeader}>
                    <View style={styles.planTitleBlock}>
                      <View style={styles.nameRow}>
                        <AppText variant="body" weight="600">
                          {plan.name}
                        </AppText>
                        {plan.badge ? (
                          <View style={styles.badge}>
                            <AppText variant="label" color={colors.accentDeep} weight="600">
                              {plan.badge}
                            </AppText>
                          </View>
                        ) : null}
                      </View>
                      <AppText variant="caption" color={colors.muted}>
                        {plan.summary}
                      </AppText>
                    </View>
                    <AppText variant="headline" color={colors.accentDeep}>
                      {plan.priceLabel}
                    </AppText>
                  </View>

                  <View style={styles.perks}>
                    {plan.perks.map((perk) => (
                      <View key={perk} style={styles.perkRow}>
                        <Check size={14} color={colors.accent} strokeWidth={2.6} />
                        <AppText variant="caption" color={colors.fg} style={styles.perkText}>
                          {perk}
                        </AppText>
                      </View>
                    ))}
                  </View>

                  <View style={[styles.radio, selectedPlan && styles.radioOn]}>
                    {selectedPlan ? <View style={styles.radioDot} /> : null}
                  </View>
                </SurfaceCard>
              </Pressable>
            );
          })}
        </View>
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  center: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
  },
  content: {
    gap: 22,
    paddingTop: 12,
    paddingBottom: 8,
  },
  hero: {
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 8,
  },
  shield: {
    width: 48,
    height: 48,
    borderRadius: 14,
    backgroundColor: colors.accentSoft,
    borderWidth: 1.5,
    borderColor: colors.accent,
    alignItems: 'center',
    justifyContent: 'center',
    marginBottom: 4,
  },
  eyebrow: {
    letterSpacing: 0.8,
    textTransform: 'uppercase',
  },
  lead: {
    maxWidth: 320,
    lineHeight: 20,
    marginTop: 2,
  },
  plans: {
    gap: 12,
  },
  planCard: {
    gap: 12,
    padding: 16,
    borderRadius: radii['2xl'],
    position: 'relative',
  },
  planIdle: {
    borderColor: colors.border,
  },
  planSelected: {
    borderColor: colors.accent,
    borderWidth: 1.5,
    backgroundColor: colors.surface,
  },
  planHeader: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 12,
    paddingRight: 28,
  },
  planTitleBlock: {
    flex: 1,
    gap: 4,
  },
  nameRow: {
    flexDirection: 'row',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: 8,
  },
  badge: {
    backgroundColor: colors.accentSoft,
    borderRadius: radii.full,
    paddingHorizontal: 8,
    paddingVertical: 3,
  },
  perks: {
    gap: 6,
  },
  perkRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  perkText: {
    flex: 1,
    lineHeight: 16,
  },
  radio: {
    position: 'absolute',
    top: 16,
    right: 16,
    width: 22,
    height: 22,
    borderRadius: 11,
    borderWidth: 1.5,
    borderColor: colors.border,
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: colors.surface,
  },
  radioOn: {
    borderColor: colors.accent,
  },
  radioDot: {
    width: 12,
    height: 12,
    borderRadius: 6,
    backgroundColor: colors.accent,
  },
  pressed: {
    opacity: 0.92,
  },
  footer: {
    paddingHorizontal: 18,
    paddingTop: 10,
    paddingBottom: 10,
    gap: 8,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: colors.border,
    backgroundColor: 'transparent',
  },
});
