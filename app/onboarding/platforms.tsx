import { useState, useEffect, useRef } from "react";
import {
  FlatList,
  Pressable,
  StyleSheet,
  View as RNView,
  Animated,
  Easing,
} from "react-native";
import * as Haptics from "expo-haptics";
import { router, Stack } from "expo-router";
import { apiPost } from "@/lib/api";
import { getOrCreateUserId } from "@/lib/user";
import {
  Platform,
  PLATFORMS as SOCIAL_PLATFORMS,
  PLATFORM_ICON,
  isBackendProvider,
} from "@/constants/Platforms";

import { Text, View } from "@/components/Themed";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import Screen from "@/components/Screen";
import GradientBackground from "@/components/GradientBackground";
import BackButton from "@/components/BackButton";
import Colors from "@/constants/Colors";
import { spacing, radii, fontFamilies } from "@/constants/Tokens";

type PlatformItem = { key: Platform; label: string };

const PLATFORMS: PlatformItem[] = SOCIAL_PLATFORMS.map(({ key, label }) => ({
  key,
  label,
}));

export default function PlatformsScreen() {
  const { bottom } = useSafeAreaInsets();
  const [selected, setSelected] = useState<Record<Platform, boolean>>(
    {} as Record<Platform, boolean>
  );

  const toggle = (key: Platform) =>
    setSelected((prev) => ({ ...prev, [key]: !prev[key] }));
  const isSelected = Object.values(selected).some(Boolean);

  const onStart = () => {
    void persistSelection().then(() => router.replace("/(tabs)/dashboard"));
  };

  async function persistSelection() {
    const userId = await getOrCreateUserId();
    const selectedProviders = (
      Object.entries(selected) as Array<[Platform, boolean]>
    )
      .filter(([, v]) => v)
      .map(([k]) => k)
      .filter(isBackendProvider);

    await Promise.all(
      selectedProviders.map((provider) =>
        apiPost<{ ok: boolean }>("/connected-accounts", {
          userId,
          provider,
          status: "connected",
        })
      )
    );
  }

  return (
    <Screen>
      <Stack.Screen options={{ headerShown: false }} />
      <GradientBackground />
      <BackButton />
      <RNView style={styles.headerCopy}>
        <Text style={styles.h1}>Where have you been posting?</Text>
        <Text style={styles.h2}>Select all platforms you use.</Text>
      </RNView>
      <FlatList
        style={{ flex: 1 }}
        data={PLATFORMS}
        keyExtractor={(item) => item.key}
        numColumns={2}
        columnWrapperStyle={{ gap: spacing.sm }}
        contentContainerStyle={{
          padding: spacing.md,
          gap: spacing.sm,
          flexGrow: 1,
          justifyContent: "center",
          paddingBottom: bottom + spacing.xl,
        }}
        showsVerticalScrollIndicator={false}
        renderItem={({ item }) => {
          const active = !!selected[item.key];
          return (
            <PlatformCard
              key={item.key}
              label={item.label}
              iconKey={item.key}
              active={active}
              onPress={() => {
                const next = !active;
                if (next) {
                  void Haptics.notificationAsync(
                    Haptics.NotificationFeedbackType.Success
                  );
                } else {
                  void Haptics.selectionAsync();
                }
                toggle(item.key);
              }}
            />
          );
        }}
      />

      <RNView style={styles.footer}>
        <Pressable
          onPress={onStart}
          disabled={!isSelected}
          style={[styles.primaryBtn, !isSelected && { opacity: 0.4 }]}
        >
          <Text style={styles.primaryText}>Start Scanning</Text>
        </Pressable>
      </RNView>
    </Screen>
  );
}

function PlatformCard({
  label,
  iconKey,
  active,
  onPress,
}: {
  label: string;
  iconKey: Platform;
  active: boolean;
  onPress: () => void;
}) {
  const scale = useRef(new Animated.Value(1)).current;
  const bg = useRef(new Animated.Value(active ? 1 : 0)).current;

  // Animate background/border on active change
  useEffect(() => {
    Animated.timing(bg, {
      toValue: active ? 1 : 0,
      duration: 200,
      easing: Easing.inOut(Easing.ease),
      useNativeDriver: false,
    }).start();
  }, [active, bg]);

  const backgroundColor = bg.interpolate({
    inputRange: [0, 1],
    outputRange: ["rgba(0,0,0,0.02)", "rgba(76,114,64,0.12)"], // Success (Fern Green) 12%
  });
  const borderColor = bg.interpolate({
    inputRange: [0, 1],
    outputRange: ["rgba(0,0,0,0.1)", Colors.light.success],
  });

  const onPressIn = () => {
    Animated.spring(scale, {
      toValue: 0.98,
      useNativeDriver: true,
      speed: 20,
      bounciness: 0,
    }).start();
  };
  const onPressOut = () => {
    Animated.spring(scale, {
      toValue: 1,
      useNativeDriver: true,
      speed: 20,
      bounciness: 6,
    }).start();
  };

  return (
    <Pressable
      onPress={onPress}
      onPressIn={onPressIn}
      onPressOut={onPressOut}
      style={{ flex: 1 }}
    >
      {/* Outer view handles color/border (JS-driven) */}
      <Animated.View
        style={[
          styles.row,
          {
            backgroundColor,
            borderColor,
          },
        ]}
      >
        {/* Inner view handles transform (native-driven) */}
        <Animated.View style={{ transform: [{ scale }] }}>
          <RNView style={styles.rowLeft}>
            <RNView style={styles.logoWrap}>{platformSvg(iconKey)}</RNView>
            <Text style={styles.rowTitle}>{label}</Text>
          </RNView>
          {active ? (
            <RNView style={styles.checkBadge}>
              <Text style={styles.checkText}>✓</Text>
            </RNView>
          ) : null}
        </Animated.View>
      </Animated.View>
    </Pressable>
  );
}

function platformSvg(key: Platform) {
  const size = 36;
  const Icon = PLATFORM_ICON[key];
  return <Icon width={size} height={size} />;
}

const styles = StyleSheet.create({
  container: { flex: 1 },
  headerCopy: {
    alignItems: "center",
    justifyContent: "center",
    paddingTop: 72,
    paddingBottom: spacing.sm,
    gap: 4,
    backgroundColor: "transparent",
  },
  h1: { color: "#FBF9F4", fontSize: 24, fontFamily: fontFamilies.bold },
  h2: { color: "#FBF9F4", opacity: 0.9 },
  row: {
    padding: spacing.md,
    borderRadius: radii.md,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: "rgba(0,0,0,0.1)",
    backgroundColor: "rgba(0,0,0,0.02)",
    aspectRatio: 1,
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
  },
  rowLeft: { flexDirection: "column", alignItems: "center", gap: spacing.sm },
  logoWrap: {
    width: 56,
    height: 56,
    alignItems: "center",
    justifyContent: "center",
  },
  rowTitle: {
    fontSize: 16,
    fontFamily: fontFamilies.semibold,
    textAlign: "center",
  },
  checkBadge: {
    position: "absolute",
    top: spacing.sm,
    right: spacing.sm,
    width: 24,
    height: 24,
    borderRadius: 12,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: Colors.light.success,
  },
  checkText: { color: "#fff", fontFamily: fontFamilies.bold },
  footer: {
    padding: spacing.md,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: "rgba(255,255,255,0.2)",
    backgroundColor: "transparent",
  },
  primaryBtn: {
    backgroundColor: Colors.light.primary,
    paddingVertical: 14,
    borderRadius: radii.md,
    alignItems: "center",
  },
  primaryText: {
    color: Colors.light.background,
    fontFamily: fontFamilies.bold,
  },
});
