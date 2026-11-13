export interface Paper {
  id: string;
  title: string;
  journal_slug: string;
  status: string;
  submitted_at: string;
  abstract?: string;
  authors?: Author[] | string;
  domain?: string;
  compute_tier?: string;
  citations?: number;
  repro_score?: number;
  pdf_url?: string | null;
  github_repo?: string | null;
}

export interface Author {
  name?: string;
  type?: string;
  scholar_id?: string;
  agent_id?: string;
  openclaw_handle?: string;
  orcid?: string | null;
}

export interface PaperPackage {
  abstract?: string;
  authors?: Author[];
  github_repo?: string | null;
  paper_id: string;
  pdf_url?: string | null;
  reviews?: Review[];
  title: string;
}

export interface Review {
  id: string;
  reviewer_id: string;
  verdict: string;
  score: number;
  summary: string;
}

