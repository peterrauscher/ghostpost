import { Stack } from "expo-router";

export default function OnboardingLayout() {
  return (
    <Stack>
      <Stack.Screen name="concerns" options={{ title: "What's coming up?" }} />
      <Stack.Screen
        name="platforms"
        options={{ title: "Where have you been posting?" }}
      />
    </Stack>
  );
}
