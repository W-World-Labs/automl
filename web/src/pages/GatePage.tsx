// Human gate: the decision surface v1 never had. Admins review the pending
// queue, leave a verdict, and move papers to accepted / rejected — which emits
// the paper.decided webhook event server-side.

import { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { api, type Paper } from '../api/client';
import { AsyncBoundary } from '../components/AsyncBoundary';
import { errorCopy } from '../lib/errors';
import { useSession } from '../lib/session';
import { usePapersPages } from './PapersPage';
import { usePageTitle } from '../lib/title';

const PENDING = new Set(['submitted', 'under_review']);

export function GatePage() {
  usePageTitle('Decision queue');
  const session = useSession();
  const q = usePapersPages();
  const pending = q.data?.pages.flatMap((p) => p.data).filter((p) => PENDING.has(p.review_status));

  if (session.data && !session.data.is_admin) {
    return <div className="state state-error">The decision queue is admin-only.</div>;
  }

  return (
    <section>
      <h1>Decision queue</h1>
      <AsyncBoundary
        isLoading={q.isLoading}
        error={q.error}
        data={pending}
        isEmpty={(items) => items.length === 0}
        emptyText="Nothing awaiting a decision."
      >
        {(items) => (
          <>
            <ul className="item-list">
              {items.map((p) => (
                <PendingRow key={p.id} paper={p} />
              ))}
            </ul>
            {q.hasNextPage && (
              <div className="load-more">
                <button className="btn btn-secondary" onClick={() => q.fetchNextPage()}>
                  Load more
                </button>
              </div>
            )}
          </>
        )}
      </AsyncBoundary>
    </section>
  );
}

function PendingRow({ paper }: { paper: Paper }) {
  const qc = useQueryClient();
  const [verdict, setVerdict] = useState<'approve' | 'reject' | 'revise'>('approve');
  const [body, setBody] = useState('');
  const [message, setMessage] = useState<string | null>(null);

  const review = useMutation({
    mutationFn: () =>
      api(`/api/v1/papers/${paper.id}/reviews`, {
        method: 'POST',
        body: JSON.stringify({ verdict, body }),
      }),
    onSuccess: () => {
      setMessage('review recorded');
      setBody('');
      qc.invalidateQueries({ queryKey: ['reviews', paper.id] });
    },
    onError: (err) => setMessage(errorCopy(err)),
  });

  const decide = useMutation({
    mutationFn: (to: string) =>
      api(`/api/v1/papers/${paper.id}`, {
        method: 'PATCH',
        body: JSON.stringify({ review_status: to }),
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['papers'] }),
    onError: (err) => setMessage(errorCopy(err)),
  });

  return (
    <li>
      <Link className="item-title" to={`/papers/${paper.id}`}>
        {paper.title}
      </Link>
      <div className="item-meta">
        <span className={`badge badge-${paper.review_status}`}>{paper.review_status}</span>
        <span>{new Date(paper.created_at).toLocaleDateString()}</span>
      </div>

      <div className="form-row" style={{ marginTop: 10 }}>
        <select value={verdict} onChange={(e) => setVerdict(e.target.value as typeof verdict)}>
          <option value="approve">approve</option>
          <option value="reject">reject</option>
          <option value="revise">revise</option>
        </select>
        <textarea
          placeholder="Review notes (markdown, optional)"
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
      </div>
      <div className="form-actions">
        <button className="btn btn-secondary" disabled={review.isPending} onClick={() => review.mutate()}>
          Submit review
        </button>
        {paper.review_status === 'submitted' && (
          <button className="btn btn-secondary" onClick={() => decide.mutate('under_review')}>
            Move to under review
          </button>
        )}
        {paper.review_status === 'under_review' && (
          <>
            <button className="btn" onClick={() => decide.mutate('accepted')}>
              Accept
            </button>
            <button className="btn" onClick={() => decide.mutate('rejected')}>
              Reject
            </button>
          </>
        )}
      </div>
      {message && <div className="item-meta">{message}</div>}
    </li>
  );
}
