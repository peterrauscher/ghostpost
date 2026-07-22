import { Tabs } from 'expo-router/js-tabs';
import { useRouter } from 'expo-router';
import { View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { BottomTabBar } from '@/components';
import { colors } from '@/theme';

export default function TabsLayout() {
  const insets = useSafeAreaInsets();
  const router = useRouter();

  return (
    <Tabs
      tabBar={({ state }) => {
        const routeName = state.routes[state.index]?.name;
        const active =
          routeName === 'scan' ? 'scan' : routeName === 'profile' ? 'profile' : 'home';
        return (
          <View
            style={{
              paddingBottom: Math.max(insets.bottom - 8, 0),
              backgroundColor: colors.surface,
            }}>
            <BottomTabBar
              active={active}
              onPress={(key) => {
                if (key === 'home') router.navigate('/(tabs)');
                if (key === 'scan') router.navigate('/(tabs)/scan');
                if (key === 'profile') router.navigate('/(tabs)/profile');
              }}
            />
          </View>
        );
      }}
      screenOptions={{ headerShown: false }}>
      <Tabs.Screen name="index" options={{ title: 'Home' }} />
      <Tabs.Screen name="scan" options={{ title: 'Scan' }} />
      <Tabs.Screen name="profile" options={{ title: 'Profile' }} />
    </Tabs>
  );
}
