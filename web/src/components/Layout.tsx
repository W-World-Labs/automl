import { NavLink, Outlet } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { api, loginHref } from '../api/client';
import { useSession } from '../lib/session';

interface Health {
  service?: string;
  version?: string;
  status?: string;
}

export function Layout() {
  const session = useSession();
  const health = useQuery({
    queryKey: ['health'],
    queryFn: () => api<Health>('/api/v1/health'),
    staleTime: 300_000,
  });

  const logout = async () => {
    await fetch('/auth/logout', { method: 'POST', credentials: 'include' });
    window.location.assign('/');
  };

  return (
    <div className="frame">
      <header className="masthead">
        <div className="container masthead-row">
          <NavLink to="/" className="journal-mark">
            Auto.ml
          </NavLink>
          <div className="session">
            {session.data?.authenticated ? (
              <>
                <span className="who">{session.data.login ?? session.data.sub}</span>
                <button className="linklike" onClick={logout}>
                  Sign out
                </button>
              </>
            ) : (
              <a href={loginHref()}>Sign in with W-ID</a>
            )}
          </div>
        </div>
        <nav className="journal-nav">
          <div className="container">
            <NavLink to="/papers">Papers</NavLink>
            <NavLink to="/forum">Forum</NavLink>
            <NavLink to="/keys">API Keys</NavLink>
            {session.data?.is_admin && <NavLink to="/gate">Decisions</NavLink>}
          </div>
        </nav>
      </header>
      <main className="container">
        <Outlet />
      </main>
      <footer className="footer container">
        <span>
          Auto.ml — reproducible research. Computation runs on contributors’ machines; the
          platform verifies and aggregates evidence.
        </span>
        <span className="version">
          {health.data?.version ? `v${health.data.version}` : ''}
        </span>
      </footer>
    </div>
  );
}
