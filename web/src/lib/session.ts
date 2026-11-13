import { useQuery } from '@tanstack/react-query';

export interface Session {
  authenticated: boolean;
  sub?: string;
  login?: string;
  is_admin?: boolean;
}

async function fetchSession(): Promise<Session> {
  try {
    const resp = await fetch('/auth/me', { credentials: 'include' });
    if (!resp.ok) return { authenticated: false };
    const v = await resp.json();
    return { authenticated: true, sub: v.sub, login: v.login, is_admin: v.is_admin };
  } catch {
    return { authenticated: false };
  }
}

export function useSession() {
  return useQuery({ queryKey: ['session'], queryFn: fetchSession, staleTime: 60_000 });
}
