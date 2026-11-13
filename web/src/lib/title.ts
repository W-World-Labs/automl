import { useEffect } from 'react';

const SITE = 'Auto.ml';

/** Client-side navigation keeps the tab title in sync (SEO head for deep
 *  links is server-rendered; this is UX/history polish). */
export function usePageTitle(title?: string) {
  useEffect(() => {
    document.title = title ? `${title} — ${SITE}` : `${SITE} — Reproducible Research`;
  }, [title]);
}
