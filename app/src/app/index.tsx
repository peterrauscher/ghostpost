import { Redirect } from 'expo-router';

import { useAppState } from '@/providers/app-state';

/**
 * Entry redirect — Stack.Protected screens own the real destinations,
 * but a stable `/` route keeps deep links and reloads predictable.
 */
export default function Index() {
  const { gate, hydrated } = useAppState();

  if (!hydrated) return null;

  switch (gate) {
    case 'welcome':
      return <Redirect href="/welcome" />;
    case 'onboarding':
      return <Redirect href="/onboarding/coming-up" />;
    case 'scan':
      return <Redirect href="/scan" />;
    case 'locked':
      return <Redirect href="/locked" />;
    case 'app':
    default:
      return <Redirect href="/(tabs)" />;
  }
}
