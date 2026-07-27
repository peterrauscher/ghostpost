/// <reference types="jest" />

import { deriveLifecycleGate, type LifecycleGate, type LifecycleInputs } from '@/domain/lifecycle';
import type { OnboardingState } from '@/domain/onboarding';
import type { OnboardingAnswers, PlatformId, ScanStatus } from '@/domain/types';
import type { ArchiveImport, Entitlement } from '@/services/api/types';

const consent = { accepted: true, version: 'v1' } as const;

function answers(platforms: PlatformId[]): OnboardingAnswers {
  return {
    comingUp: ['just_concerned'],
    concerns: ['public_image'],
    platforms,
    disclosureConsent: { ...consent },
  };
}

function onboarding(
  status: OnboardingState['status'],
  platforms: PlatformId[] = ['reddit'],
  currentStep = 4,
): OnboardingState {
  return { status, currentStep, revision: 1, answers: answers(platforms) };
}

function archive(status: ArchiveImport['status'], platform: PlatformId = 'reddit', id = `imp-${platform}`): ArchiveImport {
  return { id, platform, status };
}

function scan(status: ScanStatus['status'], id = 'scan-1'): ScanStatus {
  return {
    id,
    status,
    phase: status === 'succeeded' ? 'complete' : 'scanning',
    progress: status === 'succeeded' ? 1 : 0.4,
    message: status,
  };
}

function entitlement(reviewAccess: boolean): Entitlement {
  return {
    status: 'active',
    productId: 'free_beta',
    validFrom: '2026-01-01T00:00:00Z',
    expiresAt: null,
    scanId: reviewAccess ? 'scan-1' : null,
    capabilities: { reviewAccess, rescansRemaining: null, platformLimit: 2 },
  };
}

describe('deriveLifecycleGate (Plan 007)', () => {
  it.each<{ name: string; input: LifecycleInputs; expected: LifecycleGate }>([
    {
      name: 'unauthenticated → welcome',
      input: { authenticated: false },
      expected: 'welcome',
    },
    {
      name: 'authenticated without onboarding → onboarding',
      input: { authenticated: true },
      expected: 'onboarding',
    },
    {
      name: 'authenticated with in_progress onboarding → onboarding',
      input: { authenticated: true, onboarding: onboarding('in_progress', ['reddit'], 2) },
      expected: 'onboarding',
    },
    {
      name: 'completed onboarding with no selected reddit/x → awaiting_import',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['facebook', 'instagram']),
        imports: [archive('ready', 'facebook')],
      },
      expected: 'awaiting_import',
    },
    {
      name: 'completed onboarding without ready reddit import → awaiting_import',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('queued', 'reddit')],
      },
      expected: 'awaiting_import',
    },
    {
      name: 'both reddit and x selected but only reddit ready → awaiting_import',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit', 'x']),
        imports: [archive('ready', 'reddit'), archive('queued', 'x')],
      },
      expected: 'awaiting_import',
    },
    {
      name: 'all required reddit/x imports ready with no scan → awaiting import',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit', 'x', 'instagram']),
        imports: [archive('ready', 'reddit'), archive('ready', 'x')],
        scan: null,
      },
      expected: 'awaiting_import',
    },
    {
      name: 'ready imports with in-flight scan → scanning',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan('running'),
      },
      expected: 'scanning',
    },
    {
      name: 'terminal failed scan → awaiting_import (not app, not locked)',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan('failed'),
        entitlement: entitlement(true),
      },
      expected: 'awaiting_import',
    },
    {
      name: 'terminal cancelled scan → awaiting_import',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['x']),
        imports: [archive('ready', 'x')],
        scan: scan('cancelled'),
      },
      expected: 'awaiting_import',
    },
    {
      name: 'succeeded scan with reviewAccess → app',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit', 'x']),
        imports: [archive('ready', 'reddit'), archive('ready', 'x')],
        scan: scan('succeeded'),
        entitlement: entitlement(true),
      },
      expected: 'app',
    },
    {
      name: 'succeeded scan without reviewAccess → awaiting_import (never locked)',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan('succeeded'),
        entitlement: entitlement(false),
      },
      expected: 'awaiting_import',
    },
  ])('$name', ({ input, expected }) => {
    expect(deriveLifecycleGate(input)).toBe(expected);
  });

  it('never emits a locked gate for any launch free_beta input', () => {
    const cases: LifecycleInputs[] = [
      { authenticated: false },
      { authenticated: true, onboarding: onboarding('not_started', [], 1) },
      { authenticated: true, onboarding: onboarding('completed', ['reddit']), imports: [] },
      {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit', 'x']),
        imports: [archive('ready', 'reddit')],
      },
      {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan('queued'),
      },
      {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan('failed'),
        entitlement: entitlement(false),
      },
      {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan('succeeded'),
        entitlement: entitlement(false),
      },
      {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan('succeeded'),
        entitlement: entitlement(true),
      },
    ];

    const allowed: LifecycleGate[] = ['welcome', 'onboarding', 'awaiting_import', 'scanning', 'app'];
    for (const input of cases) {
      const gate = deriveLifecycleGate(input);
      expect(allowed).toContain(gate);
      expect(String(gate)).not.toBe('locked');
    }
  });
});
