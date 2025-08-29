import { useState } from "react";
import {
  FlatList,
  Pressable,
  StyleSheet,
  Switch,
  View as RNView,
} from "react-native";
import InstagramIcon from "@/assets/icons/Instagram.svg";
import TikTokIcon from "@/assets/icons/TikTok.svg";
import FacebookIcon from "@/assets/icons/Facebook.svg";
import XIcon from "@/assets/icons/X.svg";
import { router } from "expo-router";
import { apiPost } from "@/lib/api";
import { getOrCreateUserId } from "@/lib/user";

import { Text, View } from "@/components/Themed";

type Platform = { key: string; label: string };

const PLATFORMS: Platform[] = [
  { key: "instagram", label: "Instagram" },
  { key: "tiktok", label: "TikTok" },
  { key: "twitter", label: "Twitter/X" },
  { key: "facebook", label: "Facebook" },
];

export default function PlatformsScreen() {
  const [selected, setSelected] = useState<Record<string, boolean>>({});

  const toggle = (key: string) =>
    setSelected((prev) => ({ ...prev, [key]: !prev[key] }));
  const isSelected = Object.values(selected).some(Boolean);

  const onStart = () => {
    void persistSelection().then(() => router.replace("/(tabs)/dashboard"));
  };

  async function persistSelection() {
    const userId = await getOrCreateUserId();
    const selectedProviders = Object.entries(selected)
      .filter(([, v]) => v)
      .map(([k]) => k) as Array<
      "instagram" | "tiktok" | "twitter" | "facebook"
    >;
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
    <View style={styles.container}>
      <FlatList
        data={PLATFORMS}
        keyExtractor={(item) => item.key}
        contentContainerStyle={{
          padding: 16,
          gap: 12,
          flexGrow: 1,
          justifyContent: "center",
        }}
        renderItem={({ item }) => {
          const active = !!selected[item.key];
          return (
            <Pressable onPress={() => toggle(item.key)} style={styles.row}>
              <RNView style={styles.rowLeft}>
                <RNView style={styles.logoWrap}>{platformSvg(item.key)}</RNView>
                <RNView>
                  <Text style={styles.rowTitle}>{item.label}</Text>
                </RNView>
              </RNView>
              <Switch value={active} onValueChange={() => toggle(item.key)} />
            </Pressable>
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
    </View>
  );
}

function platformSvg(key: string) {
  const size = 22;
  switch (key) {
    case "instagram":
      return <InstagramIcon width={size} height={size} />;
    case "tiktok":
      return <TikTokIcon width={size} height={size} />;
    case "twitter":
      return <XIcon width={size} height={size} />;
    case "facebook":
      return <FacebookIcon width={size} height={size} />;
    default:
      return null;
  }
}

const styles = StyleSheet.create({
  container: { flex: 1 },
  row: {
    padding: 16,
    borderRadius: 12,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: "rgba(0,0,0,0.1)",
    backgroundColor: "rgba(0,0,0,0.02)",
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  rowLeft: { flexDirection: "row", alignItems: "center", gap: 12 },
  logoWrap: {
    width: 32,
    height: 32,
    borderRadius: 6,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: "rgba(0,0,0,0.06)",
  },
  rowTitle: { fontSize: 16, fontWeight: "600" },
  footer: {
    padding: 16,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: "rgba(0,0,0,0.1)",
  },
  primaryBtn: {
    backgroundColor: "#111827",
    paddingVertical: 14,
    borderRadius: 12,
    alignItems: "center",
  },
  primaryText: { color: "white", fontWeight: "700" },
});
