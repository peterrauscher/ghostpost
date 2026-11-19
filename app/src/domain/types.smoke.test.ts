/// <reference types="jest" />

import {
  ARCHIVE_PLATFORM_IDS,
  COMING_UP_OPTIONS,
  CONCERN_OPTIONS,
  PLATFORM_OPTIONS,
  isArchivePlatformId,
} from '@/domain/types';

describe('domain option catalogs', () => {
  it('exposes stable coming-up ids', () => {
    expect(COMING_UP_OPTIONS.map((o) => o.id).sort()).toEqual(
      [
        'college_apps',
        'friends_family',
        'job_interviews',
        'just_concerned',
        'rush',
        'something_else',
      ].sort(),
    );
  });

  it('exposes five platform preferences', () => {
    expect(PLATFORM_OPTIONS).toHaveLength(5);
    expect(PLATFORM_OPTIONS.map((p) => p.id).sort()).toEqual(
      ['facebook', 'instagram', 'reddit', 'tiktok', 'x'].sort(),
    );
  });

  it('enables Instagram and TikTok archive imports while Facebook stays coming soon', () => {
    expect([...ARCHIVE_PLATFORM_IDS].sort()).toEqual(
      ['instagram', 'reddit', 'tiktok', 'x'].sort(),
    );
    for (const platform of PLATFORM_OPTIONS) {
      expect(isArchivePlatformId(platform.id)).toBe(platform.id !== 'facebook');
    }
  });

  it('exposes concern ids used by onboarding', () => {
    expect(CONCERN_OPTIONS.map((c) => c.id).sort()).toEqual(
      [
        'controversial_topics',
        'drinking_drugs',
        'inappropriate_language',
        'negativity',
        'other',
        'political_takes',
        'public_image',
      ].sort(),
    );
  });
});
