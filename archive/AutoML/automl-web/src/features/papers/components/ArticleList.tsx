import { NavLink } from 'react-router-dom';
import { StatusLabel } from '../../../components/StatusLabel';
import { formatDate } from '../../../lib/format';
import { paperUrl } from '../../../lib/urls';
import type { Paper } from '../types';

const excerpt = (text: string) => (text.length > 260 ? `${text.slice(0, 257)}...` : text);

export function ArticleList({ papers }: { papers: Paper[] }) {
  if (papers.length === 0) {
    return (
      <div className="empty-state">
        <h3>No articles are indexed yet</h3>
        <p>Accepted and published manuscripts will appear in the current issue.</p>
      </div>
    );
  }

  return (
    <div className="article-list">
      {papers.map((paper) => (
        <article key={paper.id} className="article-item">
          <div className="article-type">Article</div>
          <div>
            <h3><NavLink to={paperUrl(paper.id)}>{paper.title}</NavLink></h3>
            {paper.abstract && (
              <p className="article-summary">{excerpt(paper.abstract)}</p>
            )}
            <p className="article-meta">
              <span>{paper.journal_slug}</span>
              <span>{formatDate(paper.submitted_at)}</span>
              {paper.domain && <span>{paper.domain}</span>}
              {typeof paper.repro_score === 'number' && <span>Repro {paper.repro_score}</span>}
              {typeof paper.citations === 'number' && <span>{paper.citations} citations</span>}
              <StatusLabel status={paper.status} />
            </p>
            <p className="article-id">{paper.id}</p>
          </div>
        </article>
      ))}
    </div>
  );
}
