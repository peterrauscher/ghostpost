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

describe('deriveLifecycleGate', () => {
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
      name: 'authenticated with not_started onboarding → onboarding',
      input: {
        authenticated: true,
        onboarding: {
          ...onboarding('not_started', [], 1),
          answers: {
            comingUp: [],
            concerns: [],
            platforms: [],
            disclosureConsent: { accepted: false, version: consent.version },
          },
        },
      },
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
      name: 'completed onboarding with no imports → awaiting_import',
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [],
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
    ...(['queued', 'running'] as const).map((status) => ({
      name: `ready imports with ${status} scan → scanning`,
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan(status),
      },
      expected: 'scanning' as const,
    })),
    ...[false, true].map((reviewAccess) => ({
      name: `failed scan with reviewAccess=${reviewAccess} → awaiting_import`,
      input: {
        authenticated: true,
        onboarding: onboarding('completed', ['reddit']),
        imports: [archive('ready', 'reddit')],
        scan: scan('failed'),
        entitlement: entitlement(reviewAccess),
      },
      expected: 'awaiting_import' as const,
    })),
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
});
