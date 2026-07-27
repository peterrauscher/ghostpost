import type { OnboardingState } from '@/domain/onboarding';
import type { ArchiveImport, Entitlement } from '@/services/api/types';
import type { ScanStatus } from '@/domain/types';

export type LifecycleGate = 'welcome' | 'onboarding' | 'awaiting_import' | 'scanning' | 'app';

export interface LifecycleInputs {
  authenticated: boolean;
  onboarding?: OnboardingState;
  imports?: ArchiveImport[];
  scan?: ScanStatus | null;
  entitlement?: Entitlement;
}

export function deriveLifecycleGate(input: LifecycleInputs): LifecycleGate {
  if (!input.authenticated) return 'welcome';
  if (!input.onboarding || input.onboarding.status !== 'completed') return 'onboarding';
  const required = input.onboarding.answers.platforms.filter((platform) => platform === 'reddit' || platform === 'x');
  const ready = new Set((input.imports ?? []).filter((item) => item.status === 'ready').map((item) => item.platform));
  if (required.length === 0 || !required.every((platform) => ready.has(platform))) return 'awaiting_import';
  if (!input.scan) return 'awaiting_import';
  if (!['succeeded', 'failed', 'cancelled'].includes(input.scan.status)) return 'scanning';
  if (input.scan.status !== 'succeeded') return 'awaiting_import';
  return input.entitlement?.capabilities.reviewAccess ? 'app' : 'awaiting_import';
}
