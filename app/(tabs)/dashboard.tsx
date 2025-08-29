import { useEffect, useMemo, useState } from "react";
import { ActivityIndicator, StyleSheet } from "react-native";

import { Text, View } from "@/components/Themed";
import { apiGet } from "@/lib/api";
import { getOrCreateUserId } from "@/lib/user";

type ConnectedAccount = {
  id: string;
  user_id: string;
  provider: "instagram" | "tiktok" | "twitter" | "facebook";
  status: string;
  connected_at: string | null;
};

export default function DashboardScreen() {
  const [loading, setLoading] = useState(true);
  const [accounts, setAccounts] = useState<ConnectedAccount[]>([]);

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const userId = await getOrCreateUserId();
        const data = await apiGet<ConnectedAccount[]>(
          `/connected-accounts/${userId}`
        );
        if (mounted) setAccounts(data ?? []);
      } catch (e) {
        console.warn(e);
      } finally {
        if (mounted) setLoading(false);
      }
    })();
    return () => {
      mounted = false;
    };
  }, []);

  const progress = useMemo(() => {
    const total = 4; // instagram, tiktok, twitter, facebook
    const connected = accounts.filter((a) => a.status === "connected").length;
    const percent = Math.round((connected / total) * 100);
    return { connected, total, percent };
  }, [accounts]);

  return (
    <View style={styles.container}>
      <Text style={styles.title}>Dashboard</Text>
      {loading ? (
        <ActivityIndicator />
      ) : (
        <>
          <Text style={styles.subtitle}>{progress.percent}% connected</Text>
          <View style={styles.cardGrid}>
            {(["instagram", "tiktok", "twitter", "facebook"] as const).map(
              (provider) => {
                const acct = accounts.find((a) => a.provider === provider);
                const status = acct?.status ?? "not_connected";
                return (
                  <View key={provider} style={styles.card}>
                    <Text style={styles.cardTitle}>{provider}</Text>
                    <Text style={styles.cardStatus}>{status}</Text>
                  </View>
                );
              }
            )}
          </View>
        </>
      )}
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
  cardGrid: {
    marginTop: 16,
    width: "100%",
    maxWidth: 480,
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 12,
    justifyContent: "center",
  },
  card: {
    width: "47%",
    padding: 12,
    borderRadius: 12,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: "rgba(0,0,0,0.1)",
    backgroundColor: "rgba(0,0,0,0.02)",
    alignItems: "center",
    gap: 6,
  },
  cardTitle: { fontWeight: "700", textTransform: "capitalize" },
  cardStatus: { opacity: 0.7 },
});
