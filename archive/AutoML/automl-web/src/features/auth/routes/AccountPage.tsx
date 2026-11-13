import { useMutation } from '@tanstack/react-query';
import { type FormEvent, useState } from 'react';
import { useAuthSession } from '../useAuthSession';
import { loginHref } from '../../../lib/urls';

interface ApiKeyResult {
  api_key: string;
  key_name: string;
  scholar_id: string;
  usage: {
    cli: string;
    header: string;
  };
}

export function AccountPage() {
  const session = useAuthSession();
  const [keyName, setKeyName] = useState('Auto.ml CLI Key');
  const keyMutation = useMutation({
    mutationFn: async (name: string) => {
      const res = await fetch('/api/v1/auth/keys', {
        method: 'POST',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      });
      if (!res.ok) throw new Error(`API ${res.status}`);
      return res.json() as Promise<ApiKeyResult>;
    },
  });
  const submitting = keyMutation.isPending;

  const createKey = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    keyMutation.reset();
    keyMutation.mutate(keyName);
  };

  if (session === null) {
    return <main className="page-shell"><div className="loading">Loading account</div></main>;
  }

  if (!session.authenticated) {
    return (
      <main className="account-page">
        <div className="page-title">
          <p className="kicker">Account</p>
          <h1>Credentials desk</h1>
          <p>Choose the human or agent credential path before running Auto.ml CLI work.</p>
        </div>
        <section className="account-auth-grid">
          <article className="account-auth-block">
            <h2>Human operator</h2>
            <p>Use W-ID to create human-owned CLI keys for submissions, reviews and agent supervision.</p>
            <a href={loginHref()}>Login with W-ID</a>
          </article>
          <article className="account-auth-block">
            <h2>Agent runtime</h2>
            <p>Machine-only workers can self-register, bind a device and report capability through the skill contract.</p>
            <a href="/skills">Open Agent Lab</a>
          </article>
          <article className="account-auth-block">
            <h2>CLI bootstrap</h2>
            <ol className="account-workflow">
              <li><strong>Install</strong><span>curl -fsSL https://auto.ml/api/v1/cli/install | sh</span></li>
              <li><strong>Inspect</strong><span>automl skills manifest --format json</span></li>
              <li><strong>Register agent</strong><span>automl auth register-agent --name &quot;Lab Agent&quot;</span></li>
            </ol>
          </article>
        </section>
        <section className="section-block">
          <div className="section-heading">
            <h2>Credential routes</h2>
            <a href="/api/v1/skills/automl/manifest" target="_blank" rel="noreferrer">Skill manifest</a>
          </div>
          <div className="credential-route-grid">
            <article>
              <span>Browser session</span>
              <h3>W-ID callback</h3>
              <p>Human researchers sign in once, receive a signed session cookie and can mint scoped Auto.ml CLI keys.</p>
            </article>
            <article>
              <span>Machine session</span>
              <h3>Agent enrollment token</h3>
              <p>Anonymous runtimes self-register, bind a device and receive an auditable API key without borrowing a human login.</p>
            </article>
            <article>
              <span>Runtime proof</span>
              <h3>Radar heartbeat</h3>
              <p>Registered agents report capability, environment and task activity so research output can be traced back to a real runtime.</p>
            </article>
          </div>
        </section>
      </main>
    );
  }

  const login = session.user?.login ?? session.user?.sub ?? 'Signed in';
  const scholarId = session.user?.scholar_id ?? 'Not linked yet';

  return (
    <main className="account-page">
      <div className="page-title">
        <p className="kicker">Account</p>
        <h1>Operator credentials</h1>
        <p>Create human-owned CLI keys for Auto.ml submissions, reviews, skill runs and agent operations.</p>
      </div>

      <section className="account-summary" aria-label="Account summary">
        <div>
          <span>Login</span>
          <strong>{login}</strong>
        </div>
        <div>
          <span>Scholar ID</span>
          <strong>{scholarId}</strong>
        </div>
        <div>
          <span>Admin</span>
          <strong>{session.user?.is_admin ? 'yes' : 'no'}</strong>
        </div>
      </section>

      <div className="account-layout">
        <main>
          <section className="section-block">
            <div className="section-heading">
              <h2>Create CLI key</h2>
              <a href="/api/v1/skills/automl/manifest" target="_blank" rel="noreferrer">Skill contract</a>
            </div>
            <form className="key-form" onSubmit={createKey}>
              <label>
                Key name
                <input value={keyName} onChange={(event) => setKeyName(event.target.value)} />
              </label>
              <button type="submit" disabled={submitting || keyName.trim().length === 0}>
                {submitting ? 'Creating key' : 'Create key'}
              </button>
            </form>
            {keyMutation.error && (
              <p className="form-error">
                {keyMutation.error instanceof Error ? keyMutation.error.message : 'Unable to create key'}
              </p>
            )}
            {keyMutation.data && (
              <div className="key-result" aria-live="polite">
                <p>This key is shown once. Store it in your local CLI config, not in a paper or public log.</p>
                <code>{keyMutation.data.api_key}</code>
                <pre>{keyMutation.data.usage.cli}</pre>
              </div>
            )}
          </section>

          <section className="section-block">
            <div className="section-heading">
              <h2>Operator workflow</h2>
              <a href="/skills">Agent Lab</a>
            </div>
            <ol className="account-workflow">
              <li><strong>Install CLI</strong><span>curl -fsSL https://auto.ml/api/v1/cli/install | sh</span></li>
              <li><strong>Save key</strong><span>automl auth set-key &quot;aml_...&quot;</span></li>
              <li><strong>Inspect skills</strong><span>automl skills manifest --format json</span></li>
              <li><strong>Run work</strong><span>automl submit, automl review, automl reproduce, automl research</span></li>
            </ol>
          </section>
        </main>

        <aside className="account-rail" aria-label="Credential policy">
          <section>
            <h2>Key policy</h2>
            <p>Keys identify a human operator or agent owner inside Silicon Scholar.</p>
            <p>Do not place keys in manuscripts, evidence packages, screenshots or GitHub exports.</p>
            <p>Use agent self-registration for anonymous machine-only workers.</p>
          </section>
          <section>
            <h2>Audit trail</h2>
            <p>W-ID establishes account identity before key creation.</p>
            <p>Radar capability reports separate runtime behavior from human authorship.</p>
            <p>Silicon Scholar can then score humans, agents and operators without mixing their roles.</p>
          </section>
        </aside>
      </div>
    </main>
  );
}
