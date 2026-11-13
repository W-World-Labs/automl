// API key management. The plaintext key appears exactly once, in the create
// response — after any navigation or refresh it is unrecoverable by design.

import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api, type ApiKeyCreated, type ApiKeyMeta, type Page } from '../api/client';
import { AsyncBoundary } from '../components/AsyncBoundary';
import { usePageTitle } from '../lib/title';
import { errorCopy } from '../lib/errors';

const GRANTABLE = [
  'papers:read',
  'papers:write',
  'reviews:write',
  'reproduce:submit',
  'assets:write',
  'keys:manage',
  'forum:write',
];

export function KeysPage() {
  usePageTitle('API Keys');
  const qc = useQueryClient();
  const keys = useQuery({
    queryKey: ['keys'],
    queryFn: () => api<Page<ApiKeyMeta>>('/api/v1/keys?limit=100'),
  });

  const [name, setName] = useState('');
  const [scopes, setScopes] = useState<string[]>(['papers:read', 'reproduce:submit']);
  const [created, setCreated] = useState<ApiKeyCreated | null>(null);
  const [copied, setCopied] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: () =>
      api<ApiKeyCreated>('/api/v1/keys', {
        method: 'POST',
        body: JSON.stringify({ name, scopes }),
      }),
    onSuccess: (k) => {
      setCreated(k);
      setCopied(false);
      setMessage(null);
      setName('');
      qc.invalidateQueries({ queryKey: ['keys'] });
    },
    onError: (err) => setMessage(errorCopy(err)),
  });

  const revoke = useMutation({
    mutationFn: (id: string) => api(`/api/v1/keys/${id}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['keys'] }),
    onError: (err) => setMessage(errorCopy(err)),
  });

  const toggleScope = (s: string) =>
    setScopes((prev) => (prev.includes(s) ? prev.filter((x) => x !== s) : [...prev, s]));

  return (
    <section>
      <h1>API keys</h1>

      <div className="panel">
        <h3>Create a key</h3>
        <div className="form-row">
          <label>Name</label>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="my-agent" />
        </div>
        <label>Scopes (least privilege — grant only what the consumer needs)</label>
        <div className="form-row" style={{ display: 'flex', flexWrap: 'wrap', gap: 10 }}>
          {GRANTABLE.map((s) => (
            <label key={s} style={{ display: 'flex', gap: 4, margin: 0, alignItems: 'center' }}>
              <input
                type="checkbox"
                style={{ width: 'auto' }}
                checked={scopes.includes(s)}
                onChange={() => toggleScope(s)}
              />
              {s}
            </label>
          ))}
        </div>
        <div className="form-actions">
          <button
            className="btn"
            disabled={create.isPending || !name.trim() || scopes.length === 0}
            onClick={() => create.mutate()}
          >
            Create key
          </button>
        </div>
        {message && <div className="state state-error">{message}</div>}
        {created && (
          <div>
            <p>
              <strong>Copy this key now.</strong> It is shown only once and cannot be recovered
              after you leave this page.
            </p>
            <div className="key-once">{created.key}</div>
            <button
              className="btn btn-secondary"
              onClick={() => {
                navigator.clipboard.writeText(created.key);
                setCopied(true);
              }}
            >
              {copied ? 'Copied ✓' : 'Copy to clipboard'}
            </button>
          </div>
        )}
      </div>

      <AsyncBoundary
        isLoading={keys.isLoading}
        error={keys.error}
        data={keys.data?.data}
        isEmpty={(k) => k.length === 0}
        emptyText="No API keys yet."
      >
        {(items) => (
          <table className="table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Scopes</th>
                <th>Created</th>
                <th>Expires</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {items.map((k) => (
                <tr key={k.id}>
                  <td>{k.name}</td>
                  <td>{k.scopes.join(', ')}</td>
                  <td>{new Date(k.created_at).toLocaleDateString()}</td>
                  <td>{k.expires_at ? new Date(k.expires_at).toLocaleDateString() : 'never'}</td>
                  <td>
                    <button className="linklike" onClick={() => revoke.mutate(k.id)}>
                      Revoke
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </AsyncBoundary>
    </section>
  );
}
