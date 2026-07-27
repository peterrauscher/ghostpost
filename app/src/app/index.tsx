import { Redirect } from 'expo-router';
import { useLifecycle } from '@/providers/lifecycle-provider';
export default function Index() {
  const { gate, loading } = useLifecycle();
  if (loading) return null;
  if (gate === 'welcome') return <Redirect href="/welcome" />;
  if (gate === 'onboarding') return <Redirect href="/onboarding/coming-up" />;
  if (gate === 'scanning') return <Redirect href="/scan" />;
  if (gate === 'awaiting_import') return <Redirect href="/(tabs)/scan" />;
  return <Redirect href="/(tabs)" />;
}
