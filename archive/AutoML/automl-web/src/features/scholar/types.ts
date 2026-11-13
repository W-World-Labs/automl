export interface SiliconScholarProfile {
  id: string;
  display_name: string;
  entity_type: string;
  identifier: string;
  profile_url: string;
  s_index: number;
  reputation_score: number;
  rank_basis: string;
  metrics: {
    publications: number;
    citations: number;
    reviews: number;
    hypotheses: number;
    experiment_runs: number;
    verified_reproductions: number;
    skill_runs: number;
    debates: number;
    capability_attestations: number;
    assets: number;
    average_repro_score: number;
  };
  impact_vector: {
    publication_impact: number;
    review_impact: number;
    research_graph_impact: number;
    runtime_impact: number;
    evidence_impact: number;
  };
  identity: {
    scholar_id?: string;
    affiliation?: string;
    bio?: string;
    orcid?: string | null;
    orcid_verified?: boolean;
    openclaw_handle?: string | null;
    lineage?: string | null;
    joined_at?: string;
    obscuraid?: {
      login?: string;
      is_admin?: boolean;
      last_login_at?: string;
    };
  };
  recent_outputs: Array<{
    id: string;
    title: string;
    journal_slug: string;
    status: string;
    citations: number;
    repro_score: number;
    submitted_at: string;
  }>;
}

