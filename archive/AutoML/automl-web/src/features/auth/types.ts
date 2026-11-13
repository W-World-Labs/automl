export interface AuthSession {
  authenticated: boolean;
  sso_configured: boolean;
  user?: {
    sub: string;
    login?: string;
    is_admin?: boolean;
    scholar_id?: string | null;
  };
}

