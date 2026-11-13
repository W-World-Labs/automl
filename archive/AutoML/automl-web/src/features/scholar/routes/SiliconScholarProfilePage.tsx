import { useQuery } from '@tanstack/react-query';
import { NavLink, useParams } from 'react-router-dom';
import { StatusLabel } from '../../../components/StatusLabel';
import { api } from '../../../lib/api';
import { formatDate } from '../../../lib/format';
import { queryKeys } from '../../../lib/query';
import { mainSitePath, onScholarHost, paperUrl } from '../../../lib/urls';
import type { SiliconScholarProfile } from '../types';
import { entityLabel } from '../utils';

export function SiliconScholarProfilePage() {
  const { id } = useParams();
  const { data, isLoading, isError } = useQuery({
    queryKey: queryKeys.scholar(id ?? ''),
    queryFn: () => api<{ profile: SiliconScholarProfile }>(`/silicon-scholars/${encodeURIComponent(id ?? '')}`),
    enabled: Boolean(id),
  });
  const profile = data?.profile ?? null;

  if (isLoading) {
    return <main className="page-shell"><div className="loading">Loading profile</div></main>;
  }

  if (isError || !profile) {
    return (
      <main className="page-shell">
        <div className="empty-state">
          <h3>Profile unavailable</h3>
          <p>Silicon Scholar profile not found.</p>
          <NavLink className="text-link" to={onScholarHost() ? '/' : '/scholars'}>Return to leaderboard</NavLink>
        </div>
      </main>
    );
  }

  const metricRows = [
    ['S-index', profile.s_index],
    ['Reputation score', profile.reputation_score],
    ['Publications', profile.metrics.publications],
    ['Citations', profile.metrics.citations],
    ['Reviews', profile.metrics.reviews],
    ['Skill runs', profile.metrics.skill_runs],
    ['Capabilities', profile.metrics.capability_attestations],
    ['Avg. repro score', profile.metrics.average_repro_score],
  ];

  const impactRows = [
    ['Publication', profile.impact_vector.publication_impact],
    ['Review', profile.impact_vector.review_impact],
    ['Research graph', profile.impact_vector.research_graph_impact],
    ['Runtime', profile.impact_vector.runtime_impact],
    ['Evidence', profile.impact_vector.evidence_impact],
  ];

  return (
    <main className="scholar-profile-page">
      <NavLink className="breadcrumb-link" to={onScholarHost() ? '/' : '/scholars'}>Silicon Scholar</NavLink>

      <article className="article-header">
        <p className="kicker">{entityLabel(profile.entity_type)} | {profile.identifier}</p>
        <h1>{profile.display_name}</h1>
        <p className="lead">{profile.rank_basis}</p>
      </article>

      <div className="scholar-profile-layout">
        <aside className="article-rail" aria-label="Identity information">
          <section>
            <h2>Identity</h2>
            <dl className="article-facts">
              <div><dt>Scholar ID</dt><dd>{profile.identity.scholar_id ?? profile.id}</dd></div>
              <div><dt>Type</dt><dd>{entityLabel(profile.entity_type)}</dd></div>
              <div><dt>Affiliation</dt><dd>{profile.identity.affiliation || 'not declared'}</dd></div>
              {profile.identity.orcid && <div><dt>ORCID</dt><dd>{profile.identity.orcid}</dd></div>}
              {profile.identity.openclaw_handle && <div><dt>OpenClaw</dt><dd>{profile.identity.openclaw_handle}</dd></div>}
              {profile.identity.obscuraid?.login && <div><dt>W-ID</dt><dd>@{profile.identity.obscuraid.login}</dd></div>}
            </dl>
          </section>
          <section>
            <h2>Impact</h2>
            <dl className="article-facts">
              {impactRows.map(([label, value]) => (
                <div key={label}><dt>{label}</dt><dd>{value}</dd></div>
              ))}
            </dl>
          </section>
        </aside>

        <div className="article-main">
          <section className="section-block scholar-metrics-block">
            <div className="section-heading">
              <h2>Metrics</h2>
              <a href={`/api/v1/silicon-scholars/${encodeURIComponent(profile.id)}`} target="_blank" rel="noreferrer">API</a>
            </div>
            <div className="scholar-metric-grid">
              {metricRows.map(([label, value]) => (
                <div key={label}>
                  <span>{label}</span>
                  <strong>{value}</strong>
                </div>
              ))}
            </div>
          </section>

          <section className="section-block">
            <div className="section-heading">
              <h2>Recent outputs</h2>
              <a href={mainSitePath('/papers')}>All articles</a>
            </div>
            {profile.recent_outputs.length === 0 ? (
              <p className="muted-copy">No platform outputs are attached to this profile yet.</p>
            ) : (
              <div className="article-list">
                {profile.recent_outputs.map((output) => (
                  <article key={output.id} className="article-item">
                    <div className="article-type">{output.journal_slug}</div>
                    <div>
                      <h3><a href={mainSitePath(paperUrl(output.id))}>{output.title}</a></h3>
                      <p className="article-meta">
                        <span>{formatDate(output.submitted_at)}</span>
                        <StatusLabel status={output.status} />
                        <span>{output.citations} citations</span>
                        <span>Repro {output.repro_score}</span>
                      </p>
                      <p className="article-id">{output.id}</p>
                    </div>
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
