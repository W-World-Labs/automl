import { useQuery } from '@tanstack/react-query';
import { useParams } from 'react-router-dom';
import {
  api,
  type Page,
  type Paper,
  type ReproduceRun,
  type ReproductionSummary,
  type Review,
} from '../api/client';
import { AsyncBoundary } from '../components/AsyncBoundary';
import { usePageTitle } from '../lib/title';

export function PaperDetailPage() {
  const { id } = useParams<{ id: string }>();
  const paper = useQuery({
    queryKey: ['paper', id],
    queryFn: () => api<Paper>(`/api/v1/papers/${id}`),
  });

  return (
    <AsyncBoundary isLoading={paper.isLoading} error={paper.error} data={paper.data}>
      {(p) => <PaperBody paper={p} />}
    </AsyncBoundary>
  );
}

function PaperBody({ paper }: { paper: Paper }) {
  usePageTitle(paper.title);
  const authors = Array.isArray(paper.authors) ? (paper.authors as unknown[]).join(', ') : '';
  return (
    <article>
      <h1>{paper.title}</h1>
      <div className="item-meta">
        <span className={`badge badge-${paper.review_status}`}>{paper.review_status}</span>
        <span className={`badge badge-${paper.code_status}`}>{paper.code_status}</span>
        {authors && <span>{authors}</span>}
        <span>{new Date(paper.created_at).toLocaleDateString()}</span>
      </div>
      {paper.abstract && <p>{paper.abstract}</p>}

      <div className="panel">
        <h3>Code pointer</h3>
        <dl className="kv">
          <dt>Repository</dt>
          <dd>
            {paper.repo_url ? (
              <a href={paper.repo_url.replace(/\.git$/, '')} target="_blank" rel="noreferrer">
                {paper.repo_url}
              </a>
            ) : (
              'provisioning…'
            )}
          </dd>
          <dt>Pinned commit</dt>
          <dd>{paper.commit_sha || '(not pinned yet)'}</dd>
          {paper.latest_commit && (
            <>
              <dt>Latest push</dt>
              <dd>{paper.latest_commit}</dd>
            </>
          )}
          <dt>Reproduce spec</dt>
          <dd>{paper.reproduce_path}</dd>
        </dl>
      </div>

      <Reproduction paperId={paper.id} />
      <Reviews paperId={paper.id} />
    </article>
  );
}

function Reproduction({ paperId }: { paperId: string }) {
  const summary = useQuery({
    queryKey: ['reproduction', paperId],
    queryFn: () => api<ReproductionSummary>(`/api/v1/papers/${paperId}/reproduction`),
  });
  const runs = useQuery({
    queryKey: ['runs', paperId],
    queryFn: () => api<Page<ReproduceRun>>(`/api/v1/reproduce/runs?paper_id=${paperId}&limit=50`),
  });

  return (
    <div className="panel">
      <h3>Reproduction</h3>
      <AsyncBoundary isLoading={summary.isLoading} error={summary.error} data={summary.data}>
        {(s) => (
          <>
            <p>
              <strong>verified-by-{s.verified_by_n}</strong> — {String(s.runs_total)} runs from{' '}
              {String(s.distinct_runners)} independent runners.
              {s.verified_by_n === 0 &&
                ' No independent confirmations yet — run `automl reproduce run` to contribute.'}
            </p>
            {s.consensus_metrics != null && (
              <p className="item-meta">consensus: {JSON.stringify(s.consensus_metrics)}</p>
            )}
          </>
        )}
      </AsyncBoundary>
      <AsyncBoundary
        isLoading={runs.isLoading}
        error={runs.error}
        data={runs.data?.data}
        isEmpty={(r) => r.length === 0}
        emptyText="No reproduce runs registered."
      >
        {(items) => (
          <table className="table">
            <thead>
              <tr>
                <th>Run</th>
                <th>Runner</th>
                <th>Status</th>
                <th>Registered</th>
              </tr>
            </thead>
            <tbody>
              {items.map((r) => (
                <tr key={r.id}>
                  <td>{r.id.slice(0, 8)}</td>
                  <td>{r.runner_principal}</td>
                  <td>
                    <span className={`badge badge-${r.status}`}>{r.status}</span>
                  </td>
                  <td>{new Date(r.created_at).toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </AsyncBoundary>
    </div>
  );
}

function Reviews({ paperId }: { paperId: string }) {
  const reviews = useQuery({
    queryKey: ['reviews', paperId],
    queryFn: () => api<Page<Review>>(`/api/v1/papers/${paperId}/reviews?limit=50`),
  });
  return (
    <div className="panel">
      <h3>Reviews</h3>
      <AsyncBoundary
        isLoading={reviews.isLoading}
        error={reviews.error}
        data={reviews.data?.data}
        isEmpty={(r) => r.length === 0}
        emptyText="No reviews yet."
      >
        {(items) => (
          <ul className="item-list">
            {items.map((r) => (
              <li key={r.id}>
                <span className={`badge badge-${r.verdict}`}>{r.verdict}</span>
                {r.body && <p>{r.body}</p>}
                <div className="item-meta">
                  <span>reviewer {r.reviewer_id.slice(0, 8)}</span>
                  <span>{new Date(r.created_at).toLocaleString()}</span>
                </div>
              </li>
            ))}
          </ul>
        )}
      </AsyncBoundary>
    </div>
  );
}
