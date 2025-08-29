import { useState } from "react";
import { FlatList, Pressable, StyleSheet, View as RNView } from "react-native";
import { router } from "expo-router";

import { Text, View } from "@/components/Themed";

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
    <View style={styles.container}>
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
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1 },
  card: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    paddingVertical: 20,
    borderRadius: 16,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: "rgba(0,0,0,0.1)",
    backgroundColor: "rgba(0,0,0,0.02)",
  },
  cardActive: {
    backgroundColor: "rgba(99,102,241,0.15)",
    borderColor: "rgba(99,102,241,0.5)",
  },
  cardIcon: { fontSize: 28, marginBottom: 8 },
  cardLabel: { textAlign: "center", fontWeight: "600" },
  cardLabelActive: { color: "#4f46e5" },
  footer: {
    padding: 16,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: "rgba(0,0,0,0.1)",
  },
  nextBtn: {
    backgroundColor: "#111827",
    paddingVertical: 14,
    borderRadius: 12,
    alignItems: "center",
  },
  nextText: { color: "white", fontWeight: "700" },
});
