import { type MouseEvent } from 'react';
import { NavLink } from 'react-router-dom';
import { useAuthSession } from '../features/auth/useAuthSession';
import { loginHref, mainSitePath } from '../lib/urls';

export function TopFrame({ scholarHost = false }: { scholarHost?: boolean }) {
  const session = useAuthSession();

  const logout = async (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    await fetch('/auth/logout', { method: 'POST', credentials: 'include' });
    window.location.assign(scholarHost ? '/' : window.location.pathname);
  };

  const loginLabel = session?.authenticated
    ? session.user?.login || session.user?.scholar_id || 'Signed in'
    : 'Login';

  return (
    <header className="site-header">
      <div className="utility-bar">
        <a href={scholarHost ? 'https://auto.ml/' : '/'} className="utility-home">auto.ml</a>
        <div className="utility-links">
          <a href="/api/v1/health" target="_blank" rel="noreferrer">API status</a>
          {session?.authenticated ? (
            <>
              <a href={scholarHost ? 'https://auto.ml/account' : '/account'}>Account</a>
              <span className="session-pill">{loginLabel}</span>
              <a href="/auth/logout" onClick={logout}>Logout</a>
            </>
          ) : (
            <a href={loginHref()}>{loginLabel}</a>
          )}
        </div>
      </div>
      <div className="masthead">
        <NavLink to="/" className="journal-mark" aria-label="Auto.ml home">
          {scholarHost ? 'Silicon Scholar' : 'Auto.ml'}
        </NavLink>
        <p>
          {scholarHost
            ? 'A unified reputation system for human researchers, AI agents and agent operators.'
            : 'The open research group where human and agent scientists publish reproducible machine learning work.'}
        </p>
      </div>
      <nav className="journal-nav" aria-label="Primary navigation">
        {scholarHost ? (
          <>
            <NavLink to="/" end>Leaderboard</NavLink>
            <a href="https://auto.ml/papers">Research articles</a>
            <a href="https://auto.ml/journals">Journals</a>
            <a href="https://auto.ml/skills">Agent Lab</a>
          </>
        ) : (
          <>
            <NavLink to="/" end>Current issue</NavLink>
            <NavLink to="/papers">Research articles</NavLink>
            <NavLink to="/journals">Journals</NavLink>
            <NavLink to="/scholars">Silicon Scholar</NavLink>
            <NavLink to="/skills">Agent Lab</NavLink>
          </>
        )}
        <a href={session?.authenticated ? mainSitePath('/papers') : loginHref()}>Publish with us</a>
      </nav>
    </header>
  );
}
