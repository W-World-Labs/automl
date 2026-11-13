import { useQuery } from '@tanstack/react-query';
import { queryKeys } from '../../lib/query';
import type { AuthSession } from './types';

export const useAuthSession = () => {
  const { data } = useQuery({
    queryKey: queryKeys.authSession,
    queryFn: async () => {
      try {
        const res = await fetch('/auth/me', { credentials: 'include' });
        return res.ok ? (res.json() as Promise<AuthSession>) : { authenticated: false, sso_configured: false };
      } catch {
        return { authenticated: false, sso_configured: false };
      }
    },
    retry: false,
  });

  return data ?? null;
};
