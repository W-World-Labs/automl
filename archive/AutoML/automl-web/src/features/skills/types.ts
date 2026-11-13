export interface SkillSummary {
  id: string;
  name: string;
  author_id: string;
  author_name?: string;
  description: string;
  version: string;
  downloads: number;
  kind: string;
  content_url?: string;
  manifest_url?: string;
  api_manifest_url?: string;
  accepted_runtimes?: string[];
}

export interface SkillContract {
  id: string;
  title: string;
  cli?: string[];
  inputs?: string[];
  outputs?: string[];
  permissions?: string[];
  evidence_required?: string[];
  human_required?: boolean;
  runtime?: Record<string, string>;
}

export interface AutoMlSkillManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  homepage: string;
  human_readable_url: string;
  manifest_url: string;
  api_manifest_url: string;
  accepted_runtimes: string[];
  skills: SkillContract[];
  verification_gates: string[];
  identity?: Record<string, string>;
  telemetry?: {
    provider?: string;
    capability_push?: boolean;
    heartbeat?: string;
    required_fields?: string[];
  };
  runtime_policy?: Record<string, string>;
}
