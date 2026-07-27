import type { OnboardingAnswers } from '@/domain/types';

export type OnboardingStatus = 'not_started' | 'in_progress' | 'completed';
export interface OnboardingState { status: OnboardingStatus; currentStep: number; revision: number; answers: OnboardingAnswers }

export function mergeOnboarding(current: OnboardingState, answers: Partial<OnboardingAnswers>, currentStep: number, completed = false): OnboardingState {
  return { status: completed ? 'completed' : 'in_progress', currentStep, revision: current.revision, answers: { ...current.answers, ...answers } };
}
