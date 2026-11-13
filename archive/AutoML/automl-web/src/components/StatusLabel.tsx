import { normalizeStatus } from '../lib/format';

export function StatusLabel({ status }: { status: string }) {
  const tone = status === 'PUBLISHED' || status === 'ACCEPTED'
    ? 'positive'
    : status.includes('REJECT')
      ? 'negative'
      : status.includes('REVIEW')
        ? 'review'
        : 'neutral';

  return <span className={`status-label status-${tone}`}>{normalizeStatus(status)}</span>;
}

