import { Redirect } from 'expo-router';
import { useOnboardingQuery } from '@/features/hooks';

export default function OnboardingIndex() {
  const onboarding = useOnboardingQuery();

  if (onboarding.isPending) return null;

  if (onboarding.data?.status === 'completed') return <Redirect href="/(tabs)/scan" />;

  if (onboarding.data?.currentStep === 3) return <Redirect href="/onboarding/platforms" />;

  if (onboarding.data?.currentStep === 2) return <Redirect href="/onboarding/concerns" />;

  return <Redirect href="/onboarding/coming-up" />;
}
