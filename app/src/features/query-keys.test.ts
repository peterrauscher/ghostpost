/// <reference types="jest" />

import { identityKeys } from '@/features/query-keys';

describe('identityKeys (Plan 007)', () => {
  it('scopes the root key with session generation and user id', () => {
    expect(identityKeys.root(3, 'user-a')).toEqual(['ghostpost', 3, 'user-a']);
    expect(identityKeys.root(4, 'user-a')).toEqual(['ghostpost', 4, 'user-a']);
    expect(identityKeys.root(3, 'user-b')).toEqual(['ghostpost', 3, 'user-b']);
  });

  it('nests resource keys under generation + user so cache cannot leak across identities', () => {
    const genA = 1;
    const genB = 2;
    const userA = 'alice';
    const userB = 'bob';

    expect(identityKeys.profile(genA, userA)).toEqual(['ghostpost', genA, userA, 'me']);
    expect(identityKeys.onboarding(genA, userA)).toEqual(['ghostpost', genA, userA, 'onboarding']);
    expect(identityKeys.imports(genA, userA)).toEqual(['ghostpost', genA, userA, 'imports']);
    expect(identityKeys.import(genA, userA, 'imp-1')).toEqual(['ghostpost', genA, userA, 'imports', 'imp-1']);
    expect(identityKeys.scanCurrent(genA, userA)).toEqual(['ghostpost', genA, userA, 'scan', 'current']);
    expect(identityKeys.scan(genA, userA, 'scan-9')).toEqual(['ghostpost', genA, userA, 'scan', 'scan-9']);
    expect(identityKeys.entitlement(genA, userA)).toEqual(['ghostpost', genA, userA, 'entitlement']);
    expect(identityKeys.dashboard(genA, userA)).toEqual(['ghostpost', genA, userA, 'dashboard', 'latest']);
    expect(identityKeys.dashboard(genA, userA, 'scan-9')).toEqual(['ghostpost', genA, userA, 'dashboard', 'scan-9']);
    expect(identityKeys.flags(genA, userA)).toEqual(['ghostpost', genA, userA, 'flags', 'latest', 'all']);
    expect(identityKeys.flags(genA, userA, 'high', 'scan-9')).toEqual([
      'ghostpost',
      genA,
      userA,
      'flags',
      'scan-9',
      'high',
    ]);
    expect(identityKeys.flag(genA, userA, 'flag-1')).toEqual(['ghostpost', genA, userA, 'flag', 'flag-1']);

    // Bumping sessionGeneration or swapping userId yields a disjoint prefix.
    const keyA = identityKeys.profile(genA, userA);
    const keyGenB = identityKeys.profile(genB, userA);
    const keyUserB = identityKeys.profile(genA, userB);
    expect(keyA[1]).not.toEqual(keyGenB[1]);
    expect(keyA[2]).not.toEqual(keyUserB[2]);
    expect(identityKeys.root(genA, userA)).not.toEqual(identityKeys.root(genB, userA));
    expect(identityKeys.root(genA, userA)).not.toEqual(identityKeys.root(genA, userB));
  });
});
