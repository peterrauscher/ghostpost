import {
  Archive,
  Check,
  CircleCheck,
  Flag,
  House,
  Search,
  SquarePlus,
  Trash2,
  UserRound,
  type LucideIcon,
} from 'lucide-react-native';
import { Pressable, StyleSheet, View } from 'react-native';

import { AppText } from '@/components/primitives/AppText';
import { SurfaceCard } from '@/components/primitives/SurfaceCard';
import { colors } from '@/theme';

type TabKey = 'home' | 'scan' | 'profile';

const tabs: { key: TabKey; label: string; Icon: LucideIcon }[] = [
  { key: 'home', label: 'home', Icon: House },
  { key: 'scan', label: 'scan', Icon: SquarePlus },
  { key: 'profile', label: 'profile', Icon: UserRound },
];

type TabBarProps = {
  active: TabKey;
  onPress: (key: TabKey) => void;
  dimmed?: boolean;
};

export function BottomTabBar({ active, onPress, dimmed = false }: TabBarProps) {
  return (
    <View style={[styles.tabBar, dimmed && { opacity: 0.62 }]}>
      {tabs.map(({ key, label, Icon }) => {
        const selected = active === key;
        const color = selected ? colors.accent : colors.tabInactive;
        return (
          <Pressable
            key={key}
            accessibilityRole="tab"
            accessibilityState={{ selected }}
            onPress={() => onPress(key)}
            style={styles.tabItem}>
            <Icon size={22} color={color} />
            <AppText variant="label" color={color} weight="600">
              {label}
            </AppText>
          </Pressable>
        );
      })}
    </View>
  );
}

type ActionProps = {
  title: string;
  subtitle: string;
  tone?: 'danger' | 'accent' | 'success';
  onPress?: () => void;
};

const toneMap = {
  danger: { icon: Trash2, color: colors.riskHigh, bg: colors.riskHighBg, title: colors.riskHigh },
  accent: { icon: Archive, color: colors.accent, bg: colors.accentSoft, title: colors.fg },
  success: { icon: Check, color: colors.riskLow, bg: colors.riskLowBg, title: colors.fg },
} as const;

export function ActionRow({ title, subtitle, tone = 'accent', onPress }: ActionProps) {
  const meta = toneMap[tone];
  const Icon = meta.icon;
  return (
    <Pressable onPress={onPress} accessibilityRole="button">
      <SurfaceCard elevated={false} style={styles.action}>
        <View style={[styles.iconWell, { backgroundColor: meta.bg }]}>
          <Icon size={18} color={meta.color} />
        </View>
        <View style={styles.actionCopy}>
          <AppText variant="bodyRegular" color={meta.title} weight="600" style={{ fontSize: 14 }}>
            {title}
          </AppText>
          <AppText variant="caption" color={colors.muted} weight="400">
            {subtitle}
          </AppText>
        </View>
      </SurfaceCard>
    </Pressable>
  );
}

type HelpProps = {
  title: string;
  description: string;
  icon: 'search' | 'flag' | 'trash' | 'check';
};

const helpIcons = {
  search: Search,
  flag: Flag,
  trash: Trash2,
  check: CircleCheck,
} as const;

export function HelpStepRow({ title, description, icon }: HelpProps) {
  const Icon = helpIcons[icon];
  return (
    <SurfaceCard elevated={false} style={styles.help}>
      <View style={styles.helpIcon}>
        <Icon size={22} color={colors.accent} />
      </View>
      <View style={styles.helpCopy}>
        <AppText variant="bodyRegular" color={colors.fg} weight="700" style={{ fontSize: 14 }}>
          {title}
        </AppText>
        <AppText variant="caption" color={colors.muted} weight="400" style={{ lineHeight: 17 }}>
          {description}
        </AppText>
      </View>
    </SurfaceCard>
  );
}

const styles = StyleSheet.create({
  tabBar: {
    height: 64,
    backgroundColor: colors.surface,
    borderTopWidth: 1,
    borderTopColor: colors.border,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-around',
    paddingTop: 6,
    paddingBottom: 10,
    paddingHorizontal: 8,
  },
  tabItem: {
    width: 72,
    height: 46,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 2,
  },
  action: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    height: 60,
    padding: 12,
  },
  iconWell: {
    width: 36,
    height: 36,
    borderRadius: 10,
    alignItems: 'center',
    justifyContent: 'center',
  },
  actionCopy: {
    gap: 2,
    flex: 1,
  },
  help: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    padding: 12,
    minHeight: 64,
  },
  helpIcon: {
    width: 40,
    height: 40,
    borderRadius: 12,
    backgroundColor: colors.accentSoft,
    alignItems: 'center',
    justifyContent: 'center',
  },
  helpCopy: {
    flex: 1,
    gap: 2,
  },
});
