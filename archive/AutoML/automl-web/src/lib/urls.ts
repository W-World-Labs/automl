export const paperUrl = (paperId: string) => `/papers/${encodeURIComponent(paperId)}`;

export const onScholarHost = () => window.location.hostname === 'scholar.auto.ml';

export const currentReturnTo = () => {
  const path = `${window.location.pathname}${window.location.search}${window.location.hash}`;
  return onScholarHost() ? `${window.location.origin}${path}` : path;
};

export const loginHref = () => `/auth/login?next=${encodeURIComponent(currentReturnTo())}`;

export const scholarProfilePath = (profileId: string) =>
  onScholarHost() ? `/${encodeURIComponent(profileId)}` : `/scholars/${encodeURIComponent(profileId)}`;

export const mainSitePath = (path: string) => (onScholarHost() ? `https://auto.ml${path}` : path);

export const manuscriptHtmlUrl = (url: string) => {
  const parsed = new URL(url, window.location.origin);
  const path = parsed.pathname;
  if (!path.includes('/papers/') || !path.endsWith('.pdf')) return null;
  return `${path.replace(/\.pdf$/, '.html')}?v=20260516-ai2`;
};

export const readablePdfUrl = (url: string) => {
  const [path] = url.split('?');
  if (!path.includes('/papers/') || !path.endsWith('.pdf')) return url;
  return `${url}${url.includes('?') ? '&' : '?'}v=20260516`;
};

