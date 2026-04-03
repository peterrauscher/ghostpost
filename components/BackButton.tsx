import React from "react";
import { Pressable, StyleSheet } from "react-native";
import { router } from "expo-router";
import { Text, View } from "@/components/Themed";
import { spacing, radii, fontFamilies } from "@/constants/Tokens";
import { useSafeAreaInsets } from "react-native-safe-area-context";

export default function BackButton({ onPress }: { onPress?: () => void }) {
  const { top } = useSafeAreaInsets();
  return (
    <View style={[styles.wrap, { top }]}>
      <Pressable
        onPress={onPress ?? (() => router.back())}
        style={({ pressed }) => [styles.btn, pressed && { opacity: 0.8 }]}
        hitSlop={8}
      >
        <Text style={styles.label}>←</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: {
    position: "absolute",
    backgroundColor: "transparent",
    left: spacing.md,
    zIndex: 100,
  },
  btn: {
    width: 36,
    height: 36,
    borderRadius: radii.pill,
    alignItems: "center",
    justifyContent: "center",
  },
  label: {
    color: "#fff",
    fontFamily: fontFamilies.bold,
    fontSize: 18,
    lineHeight: 18,
  },
});
