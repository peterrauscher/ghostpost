import { Stack } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import * as SplashScreen from 'expo-splash-screen';
import { useFonts } from 'expo-font';
import { useEffect } from 'react';

import { AppStateProvider, useAppState } from '@/providers/app-state';
import { QueryProvider } from '@/providers/query-provider';

import '@/global.css';

SplashScreen.preventAutoHideAsync();

function RootNavigator() {
  const { hydrated, gate } = useAppState();
  const [fontsLoaded, fontError] = useFonts({
    'SN Pro': require('@/assets/fonts/SNPro-VariableFont_wght.ttf'),
  });

  const ready = hydrated && (fontsLoaded || !!fontError);

  useEffect(() => {
    if (ready) {
      SplashScreen.hideAsync();
    }
  }, [ready]);

  if (!ready) return null;

  return (
    <>
      <StatusBar style="dark" />
      <Stack screenOptions={{ headerShown: false, animation: 'fade' }}>
        <Stack.Screen name="index" />

        <Stack.Protected guard={gate === 'welcome'}>
          <Stack.Screen name="welcome" />
          <Stack.Screen name="login" />
        </Stack.Protected>

        <Stack.Protected guard={gate === 'onboarding'}>
          <Stack.Screen name="onboarding" />
        </Stack.Protected>

        <Stack.Protected guard={gate === 'scan'}>
          <Stack.Screen name="scan" />
        </Stack.Protected>

        <Stack.Protected guard={gate === 'locked'}>
          <Stack.Screen name="locked" />
        </Stack.Protected>

        <Stack.Protected guard={gate === 'app'}>
          <Stack.Screen name="(tabs)" />
          <Stack.Screen name="review/index" options={{ animation: 'slide_from_right' }} />
          <Stack.Screen name="review/[id]" options={{ animation: 'slide_from_right' }} />
        </Stack.Protected>
      </Stack>
    </>
  );
}

export default function RootLayout() {
  return (
    <QueryProvider>
      <AppStateProvider>
        <RootNavigator />
      </AppStateProvider>
    </QueryProvider>
  );
}
