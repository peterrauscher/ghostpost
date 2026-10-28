/// <reference types="jest" />

import { QueryClient } from '@tanstack/react-query';
import { identityKeys } from '@/features/query-keys';

// Reuse IDs across resources so fixture prefixes cannot hide namespace collisions.
const resources: [string, (generation: number, userId: string) => readonly unknown[]][] = [
  ['profile', identityKeys.profile],
  ['onboarding', identityKeys.onboarding],
  ['imports', identityKeys.imports],
  ['first import', (g, u) => identityKeys.import(g, u, 'id-1')],
  ['second import', (g, u) => identityKeys.import(g, u, 'id-2')],
  ['current scan', identityKeys.scanCurrent],
  ['first scan', (g, u) => identityKeys.scan(g, u, 'id-1')],
  ['second scan', (g, u) => identityKeys.scan(g, u, 'id-2')],
  ['entitlement', identityKeys.entitlement],
  ['latest dashboard', identityKeys.dashboard],
  ['first scan dashboard', (g, u) => identityKeys.dashboard(g, u, 'id-1')],
  ['second scan dashboard', (g, u) => identityKeys.dashboard(g, u, 'id-2')],
  ['latest flags', identityKeys.flags],
  ['latest high-risk flags', (g, u) => identityKeys.flags(g, u, 'high')],
  ['first scan flags', (g, u) => identityKeys.flags(g, u, 'all', 'id-1')],
  ['second scan flags', (g, u) => identityKeys.flags(g, u, 'all', 'id-2')],
  ['first scan high-risk flags', (g, u) => identityKeys.flags(g, u, 'high', 'id-1')],
  ['first flag', (g, u) => identityKeys.flag(g, u, 'id-1')],
  ['second flag', (g, u) => identityKeys.flag(g, u, 'id-2')],
];

describe('identity-scoped query cache', () => {
  it('keeps resources and filters separate across users and session generations', () => {
    const client = new QueryClient({ defaultOptions: { queries: { gcTime: Infinity } } });
    const identities = [
      { generation: 1, userId: 'alice' },
      { generation: 2, userId: 'alice' },
      { generation: 1, userId: 'bob' },
    ];

    try {
      for (const { generation, userId } of identities) {
        for (const [resource, key] of resources) {
          client.setQueryData(key(generation, userId), { resource, generation, userId });
        }
      }

      for (const { generation, userId } of identities) {
        for (const [resource, key] of resources) {
          expect(client.getQueryData(key(generation, userId))).toEqual({ resource, generation, userId });
        }
      }
    } finally {
      client.clear();
    }
  });
});
