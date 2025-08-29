import { StyleSheet, Pressable } from "react-native";
import { router } from "expo-router";

import { Text, View } from "@/components/Themed";

export default function SplashScreen() {
  const onGetStarted = () => {
    router.push("/onboarding/concerns");
  };

  return (
    <View style={styles.container}>
      <Text style={styles.logo}>👻 GhostPost</Text>
      <Text style={styles.tagline}>Your past doesn't have to haunt you</Text>
      <Text style={styles.subtext}>
        Clean up your socials before they matter
      </Text>

      <Pressable
        onPress={onGetStarted}
        style={({ pressed }) => [styles.cta, pressed && { opacity: 0.8 }]}
      >
        <Text style={styles.ctaText}>Get Started</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    padding: 24,
    backgroundColor: "#1f2937",
  },
  logo: {
    fontSize: 48,
    marginBottom: 12,
  },
  tagline: {
    fontSize: 20,
    fontWeight: "bold",
    color: "white",
    textAlign: "center",
  },
  subtext: {
    fontSize: 14,
    color: "rgba(255,255,255,0.8)",
    marginTop: 6,
    marginBottom: 24,
    textAlign: "center",
  },
  cta: {
    backgroundColor: "#111827",
    paddingHorizontal: 24,
    paddingVertical: 12,
    borderRadius: 12,
  },
  ctaText: {
    color: "white",
    fontSize: 16,
    fontWeight: "600",
  },
});
