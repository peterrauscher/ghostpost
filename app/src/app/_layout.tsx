import { Stack } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import * as SplashScreen from 'expo-splash-screen';
import { useFonts } from 'expo-font';
import { useEffect } from 'react';
import { AuthProvider } from '@/providers/auth-provider';
import { LifecycleProvider, useLifecycle } from '@/providers/lifecycle-provider';
import { QueryProvider } from '@/providers/query-provider';
import '@/global.css';

SplashScreen.preventAutoHideAsync();

function RootNavigator() {
  const { gate, loading } = useLifecycle();
  const [fontsLoaded, fontError] = useFonts({ 'SN Pro': require('@/assets/fonts/SNPro-VariableFont_wght.ttf') });
  const ready = !loading && (fontsLoaded || !!fontError);
  useEffect(() => { if (ready) void SplashScreen.hideAsync(); }, [ready]);
  if (!ready) return null;
  return <><StatusBar style="dark" /><Stack screenOptions={{ headerShown: false, animation: 'fade' }}><Stack.Screen name="index" /><Stack.Screen name="auth/callback" /><Stack.Protected guard={gate === 'welcome'}><Stack.Screen name="welcome" /><Stack.Screen name="login" /></Stack.Protected><Stack.Protected guard={gate === 'onboarding'}><Stack.Screen name="onboarding" /></Stack.Protected><Stack.Protected guard={gate === 'awaiting_import' || gate === 'app'}><Stack.Screen name="(tabs)" /></Stack.Protected><Stack.Protected guard={gate === 'scanning'}><Stack.Screen name="scan" /></Stack.Protected><Stack.Protected guard={gate === 'app'}><Stack.Screen name="review/index" options={{ animation: 'slide_from_right' }} /><Stack.Screen name="review/[id]" options={{ animation: 'slide_from_right' }} /></Stack.Protected></Stack></>;
}
export default function RootLayout() { return <QueryProvider><AuthProvider><LifecycleProvider><RootNavigator /></LifecycleProvider></AuthProvider></QueryProvider>; }
