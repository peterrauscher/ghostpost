import AsyncStorage from "@react-native-async-storage/async-storage";

const STORAGE_KEY = "ghostpost_user_id";

export async function getOrCreateUserId(): Promise<string> {
  const existing = await AsyncStorage.getItem(STORAGE_KEY);
  if (existing) return existing;
  const id = crypto.randomUUID();
  await AsyncStorage.setItem(STORAGE_KEY, id);
  return id;
}
