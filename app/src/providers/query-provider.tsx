import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { type ReactNode, useState } from 'react';

function makeQueryClient() {
  return new QueryClient({ defaultOptions: { queries: { staleTime: 30_000, gcTime: 60 * 60_000, retry: 1 } } });
}

export function QueryProvider({ children }: { children: ReactNode }) {
  const [client] = useState(makeQueryClient);
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}
