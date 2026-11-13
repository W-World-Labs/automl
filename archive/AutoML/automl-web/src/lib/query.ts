import { QueryClient } from '@tanstack/react-query';

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 30_000,
    },
  },
});

export const queryKeys = {
  authSession: ['auth-session'] as const,
  health: ['health'] as const,
  stats: ['stats'] as const,
  papers: (limit: number) => ['papers', limit] as const,
  paper: (id: string) => ['paper', id] as const,
  paperPackage: (id: string) => ['paper-package', id] as const,
  manuscriptHtml: (url: string | null) => ['manuscript-html', url] as const,
  scholars: ['silicon-scholars'] as const,
  scholar: (id: string) => ['silicon-scholar', id] as const,
  skills: ['skills'] as const,
  skillManifest: (id: string) => ['skill-manifest', id] as const,
};
