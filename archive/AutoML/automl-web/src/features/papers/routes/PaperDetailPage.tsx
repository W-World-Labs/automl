import { useQuery } from '@tanstack/react-query';
import { NavLink, useParams } from 'react-router-dom';
import { StatusLabel } from '../../../components/StatusLabel';
import { api } from '../../../lib/api';
import { formatDate } from '../../../lib/format';
import { queryKeys } from '../../../lib/query';
import { manuscriptHtmlUrl, readablePdfUrl } from '../../../lib/urls';
import type { Paper, PaperPackage } from '../types';
import { authorLabel, normalizeAuthors } from '../utils';

export function PaperDetailPage() {
  const { id } = useParams();
  const paperId = id ?? '';
  const encodedPaperId = encodeURIComponent(paperId);
  const paperQuery = useQuery({
    queryKey: queryKeys.paper(paperId),
    queryFn: () => api<{ paper: Paper }>(`/papers/${encodedPaperId}`),
    enabled: Boolean(id),
  });
  const packageQuery = useQuery({
    queryKey: queryKeys.paperPackage(paperId),
    queryFn: () => api<PaperPackage>(`/papers/${encodedPaperId}/package`),
    enabled: Boolean(id),
    retry: false,
  });

  const paper = paperQuery.data?.paper ?? null;
  const paperPackage = packageQuery.data ?? null;
  const pdfUrl = paper?.pdf_url ?? paperPackage?.pdf_url ?? null;
  const htmlUrl = pdfUrl ? manuscriptHtmlUrl(pdfUrl) : null;
  const manuscriptQuery = useQuery({
    queryKey: queryKeys.manuscriptHtml(htmlUrl),
    queryFn: async () => {
      const htmlResponse = await fetch(htmlUrl ?? '');
      if (!htmlResponse.ok) return null;
      return htmlResponse.text();
    },
    enabled: Boolean(htmlUrl),
    retry: false,
  });
  const manuscriptHtml = manuscriptQuery.data ?? null;

  if (paperQuery.isLoading) {
    return <main className="page-shell"><div className="loading">Loading article</div></main>;
  }

  if (paperQuery.isError || !paper) {
    return (
      <main className="page-shell">
        <div className="empty-state">
          <h3>Article unavailable</h3>
          <p>Article not found or unavailable.</p>
          <NavLink className="text-link" to="/papers">Return to articles</NavLink>
        </div>
      </main>
    );
  }

  const authors = normalizeAuthors(paper.authors ?? paperPackage?.authors);
  const abstract = paper.abstract ?? paperPackage?.abstract ?? '';
  const renderedPdfUrl = pdfUrl ? readablePdfUrl(pdfUrl) : null;
  const githubRepo = paper.github_repo ?? paperPackage?.github_repo ?? null;
  const reviews = paperPackage?.reviews ?? [];

  return (
    <main className="article-page">
      <NavLink className="breadcrumb-link" to="/papers">Research articles</NavLink>

      <article className="article-header">
        <p className="kicker">{paper.journal_slug} | Article | {paper.id}</p>
        <h1>{paper.title}</h1>
        {authors.length > 0 && (
          <p className="byline">
            {authors.map((author, index) => (
              <span key={`${author.name ?? 'author'}-${index}`}>{authorLabel(author) || 'Unnamed author'}</span>
            ))}
          </p>
        )}
        <p className="article-meta article-meta-large">
          <span>{formatDate(paper.submitted_at)}</span>
          <StatusLabel status={paper.status} />
        </p>
      </article>

      <div className="article-layout">
        <aside className="article-rail" aria-label="Article information">
          <section>
            <h2>Article info</h2>
            <dl className="article-facts">
              <div><dt>Domain</dt><dd>{paper.domain ?? 'Uncategorized'}</dd></div>
              <div><dt>Compute tier</dt><dd>{paper.compute_tier ?? 'not declared'}</dd></div>
              <div><dt>Repro score</dt><dd>{paper.repro_score ?? 0}</dd></div>
              <div><dt>Citations</dt><dd>{paper.citations ?? 0}</dd></div>
            </dl>
          </section>
          <section>
            <h2>Resources</h2>
            {renderedPdfUrl && <a className="text-link" href={renderedPdfUrl} target="_blank" rel="noreferrer">Download PDF</a>}
            {githubRepo && <a className="text-link" href={githubRepo} target="_blank" rel="noreferrer">Source repository</a>}
            <a className="text-link" href={`/api/v1/papers/${encodeURIComponent(paper.id)}/package`} target="_blank" rel="noreferrer">Package JSON</a>
          </section>
        </aside>

        <div className="article-main">
          <section className="abstract-block">
            <h2>Abstract</h2>
            <p>{abstract || 'No abstract has been recorded for this article.'}</p>
          </section>

          <section className="reader-block">
            <div className="reader-heading">
              <h2>Full text</h2>
              {renderedPdfUrl && <a href={renderedPdfUrl} target="_blank" rel="noreferrer">Download PDF</a>}
            </div>
            {manuscriptHtml ? (
              <div className="manuscript-html" dangerouslySetInnerHTML={{ __html: manuscriptHtml }} />
            ) : (
              <div className="empty-state">
                <h3>No HTML manuscript</h3>
                <p>This article does not yet include a web-rendered manuscript. Use the PDF download link for now.</p>
              </div>
            )}
          </section>

          <section className="review-block">
            <h2>Peer review</h2>
            {reviews.length === 0 ? (
              <p className="muted-copy">No review reports are attached to this package yet.</p>
            ) : (
              <div className="review-list">
                {reviews.map((review) => (
                  <article key={review.id} className="review-item">
                    <p className="article-meta">
                      <span>{review.reviewer_id}</span>
                      <span>{review.verdict}</span>
                      <span>Score {review.score}</span>
                    </p>
                    <p>{review.summary}</p>
                  </article>
                ))}
              </div>
            )}
          </section>
        </div>
      </div>
    </main>
  );
}
