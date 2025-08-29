import { StyleSheet } from "react-native";

import { Text, View } from "@/components/Themed";

export default function ReviewScreen() {
  return (
    <View style={styles.container}>
      <Text style={styles.title}>Review</Text>
      <Text style={styles.subtitle}>
        Swipe interface for flagged posts will appear here.
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    padding: 24,
  },
  title: {
    fontSize: 24,
    fontWeight: "bold",
    marginBottom: 8,
  },
  subtitle: {
    fontSize: 14,
    opacity: 0.7,
    textAlign: "center",
  },
});
