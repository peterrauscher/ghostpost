import { StyleSheet, Pressable } from "react-native";
import React from "react";
import * as Haptics from "expo-haptics";
import { router } from "expo-router";

import { Text, View } from "@/components/Themed";
import GradientBackground from "@/components/GradientBackground";
import Colors from "@/constants/Colors";
import { spacing, radii, fontFamilies, fontSizes } from "@/constants/Tokens";
import LanderGraphic from "@/components/LanderGraphic";

export default function SplashScreen() {
  const onGetStarted = () => {
    Haptics.selectionAsync();
    router.push("/onboarding/concerns");
  };

  return (
    <View style={styles.container}>
      <GradientBackground />
      <View style={styles.flexGrow}></View>
      <LanderGraphic height={300} />
      <View style={styles.landerCopy}>
        <Text style={styles.tagline}>Welcome to GhostPost</Text>
        <Text style={styles.subtext}>
          Your social media history doesn't have to haunt you.
        </Text>
      </View>
      <View style={styles.landerActions}>
        <Pressable
          onPress={onGetStarted}
          style={({ pressed }) => [
            styles.cta,
            pressed && {
              transform: [{ scale: 0.95 }],
              backgroundColor: "#352c7c",
            },
          ]}
        >
          <Text style={styles.ctaText}>Sign Up</Text>
        </Pressable>

        <Pressable
          onPress={() => router.push("/(tabs)/dashboard")}
          style={({ pressed }) => [pressed && { opacity: 0.8 }]}
        >
          <Text style={styles.secondaryLinkText}>
            I already have an account
          </Text>
        </Pressable>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "flex-start",
    padding: spacing.lg,
    backgroundColor: "transparent",
  },
  flexGrow: {
    flexGrow: 1,
  },
  landerCopy: {
    gap: spacing.xs,
    width: "100%",
    backgroundColor: "transparent",
    marginTop: "30%",
    alignItems: "center",
    justifyContent: "center",
    marginBottom: spacing.xl,
  },
  landerActions: {
    gap: spacing.lg,
    backgroundColor: "transparent",
    alignItems: "center",
    justifyContent: "center",
    marginBottom: spacing.xl,
  },
  tagline: {
    fontSize: fontSizes.xxl,
    color: "#FBF9F4",
    textAlign: "center",
    fontFamily: fontFamilies.bold,
  },
  subtext: {
    fontSize: fontSizes.md,
    color: "#FBF9F4",
    textAlign: "center",
    width: "80%",
  },
  checkboxRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
  },
  checkbox: {
    width: 22,
    height: 22,
    borderRadius: 11,
    borderWidth: 2,
    borderColor: "#FBF9F4",
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: "transparent",
  },
  checkboxChecked: {
    borderColor: "#FBF9F4",
  },
  checkboxDot: {
    width: 10,
    height: 10,
    borderRadius: 5,
    backgroundColor: "#FBF9F4",
  },
  checkboxLabel: {
    color: "#FBF9F4",
  },
  cta: {
    backgroundColor: Colors.light.primary,
    paddingHorizontal: spacing.lg,
    paddingVertical: spacing.sm,
    borderRadius: radii.md,
  },
  ctaText: {
    color: Colors.light.background,
    fontSize: fontSizes.md,
    fontFamily: fontFamilies.semibold,
  },
  secondaryLinkText: {
    color: "#FBF9F4",
    fontFamily: fontFamilies.semibold,
    fontSize: fontSizes.md,
  },
});
