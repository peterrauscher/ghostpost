import { useState } from "react";
import { FlatList, Pressable, StyleSheet, View as RNView } from "react-native";
import { router, Stack } from "expo-router";

import { Text, View } from "@/components/Themed";
import Screen from "@/components/Screen";
import GradientBackground from "@/components/GradientBackground";
import BackButton from "@/components/BackButton";
import { spacing } from "@/constants/Tokens";
import Colors from "@/constants/Colors";

type Concern = { key: string; label: string; icon: string };

const CONCERNS: Concern[] = [
  { key: "college", label: "College Applications", icon: "🎓" },
  { key: "internship", label: "Internship Hunt", icon: "💼" },
  { key: "greek", label: "Greek Life Rush", icon: "🏫" },
  { key: "relationship", label: "New Relationship", icon: "💕" },
  { key: "athletics", label: "Athletic Recruitment", icon: "🎯" },
  { key: "firstjob", label: "First Real Job", icon: "👔" },
];

export default function ConcernsScreen() {
  const [selected, setSelected] = useState<Record<string, boolean>>({});

  const toggle = (key: string) => {
    setSelected((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const onNext = () => {
    router.push("/onboarding/platforms");
  };

  const isSelected = Object.values(selected).some(Boolean);

  return (
    <Screen>
      <Stack.Screen options={{ headerShown: false }} />
      <GradientBackground />
      <BackButton />
      <RNView style={styles.headerCopy}>
        <Text style={styles.h1}>What's on the horizon?</Text>
        <Text style={styles.h2}>Pick anything that you're worried about.</Text>
      </RNView>
      <FlatList
        data={CONCERNS}
        keyExtractor={(item) => item.key}
        numColumns={2}
        columnWrapperStyle={{ gap: 12 }}
        contentContainerStyle={{
          gap: 12,
          padding: 16,
          flexGrow: 1,
          justifyContent: "center",
        }}
        renderItem={({ item }) => {
          const active = !!selected[item.key];
          return (
            <Pressable
              onPress={() => toggle(item.key)}
              style={[styles.card, active && styles.cardActive]}
            >
              <Text style={styles.cardIcon}>{item.icon}</Text>
              <Text
                style={[styles.cardLabel, active && styles.cardLabelActive]}
              >
                {item.label}
              </Text>
            </Pressable>
          );
        }}
      />
      <RNView style={styles.footer}>
        <Pressable
          onPress={onNext}
          disabled={!isSelected}
          style={[styles.nextBtn, !isSelected && { opacity: 0.4 }]}
        >
          <Text style={styles.nextText}>Next</Text>
        </Pressable>
      </RNView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1 },
  headerCopy: {
    alignItems: "center",
    justifyContent: "center",
    gap: spacing.xs,
    backgroundColor: "transparent",
  },
  h1: { color: "#FBF9F4", fontSize: 24, fontWeight: "700" },
  h2: { color: "#FBF9F4", opacity: 0.9 },
  card: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    paddingVertical: 20,
    borderRadius: 16,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: Colors.light.background,
    backgroundColor: Colors.light.background,
  },
  cardActive: {
    backgroundColor: Colors.light.secondary,
    borderColor: Colors.light.secondary,
  },
  cardIcon: { fontSize: 28, marginBottom: 8 },
  cardLabel: { textAlign: "center", fontWeight: "600" },
  cardLabelActive: { color: "#4f46e5" },
  footer: {
    padding: 16,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: "rgba(255,255,255,0.2)",
    backgroundColor: "transparent",
  },
  nextBtn: {
    backgroundColor: "#111827",
    paddingVertical: 14,
    borderRadius: 12,
    alignItems: "center",
  },
  nextText: { color: "#FBF9F4", fontWeight: "700" },
});
