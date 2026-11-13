import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { api, type Category, type Page, type Thread } from '../api/client';
import { AsyncBoundary } from '../components/AsyncBoundary';
import { usePageTitle } from '../lib/title';
import { errorCopy } from '../lib/errors';

export function ForumPage() {
  usePageTitle('Forum');
  const categories = useQuery({
    queryKey: ['forum', 'categories'],
    queryFn: () => api<Page<Category>>('/api/v1/forum/categories?limit=50'),
  });
  const [selected, setSelected] = useState<string | null>(null);

  return (
    <section>
      <h1>Forum</h1>
      <AsyncBoundary
        isLoading={categories.isLoading}
        error={categories.error}
        data={categories.data?.data}
        isEmpty={(c) => c.length === 0}
      >
        {(cats) => (
          <>
            <div className="form-row" style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              <button
                className={selected === null ? 'btn' : 'btn btn-secondary'}
                onClick={() => setSelected(null)}
              >
                All
              </button>
              {cats.map((c) => (
                <button
                  key={c.id}
                  className={selected === c.id ? 'btn' : 'btn btn-secondary'}
                  title={c.description}
                  onClick={() => setSelected(c.id)}
                >
                  {c.name}
                </button>
              ))}
            </div>
            <NewThread categories={cats} />
            <ThreadList categoryId={selected} />
          </>
        )}
      </AsyncBoundary>
    </section>
  );
}

function ThreadList({ categoryId }: { categoryId: string | null }) {
  const threads = useQuery({
    queryKey: ['forum', 'threads', categoryId],
    queryFn: () =>
      api<Page<Thread>>(
        `/api/v1/forum/threads?limit=50${categoryId ? `&category_id=${categoryId}` : ''}`,
      ),
  });
  return (
    <AsyncBoundary
      isLoading={threads.isLoading}
      error={threads.error}
      data={threads.data?.data}
      isEmpty={(t) => t.length === 0}
      emptyText="No threads yet — start one."
    >
      {(items) => (
        <ul className="item-list">
          {items.map((t) => (
            <li key={t.id}>
              <Link className="item-title" to={`/forum/threads/${t.id}`}>
                {t.title}
              </Link>
              <div className="item-meta">
                <span>updated {new Date(t.updated_at).toLocaleString()}</span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </AsyncBoundary>
  );
}

function NewThread({ categories }: { categories: Category[] }) {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [categoryId, setCategoryId] = useState(categories[0]?.id ?? '');
  const [message, setMessage] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: () =>
      api<Thread>('/api/v1/forum/threads', {
        method: 'POST',
        body: JSON.stringify({ category_id: categoryId, title, body: body || undefined }),
      }),
    onSuccess: () => {
      setOpen(false);
      setTitle('');
      setBody('');
      setMessage(null);
      qc.invalidateQueries({ queryKey: ['forum', 'threads'] });
    },
    onError: (err) => setMessage(errorCopy(err)),
  });

  if (!open) {
    return (
      <div className="form-actions">
        <button className="btn" onClick={() => setOpen(true)}>
          New thread
        </button>
      </div>
    );
  }
  return (
    <div className="panel">
      <h3>New thread</h3>
      <div className="form-row">
        <label>Category</label>
        <select value={categoryId} onChange={(e) => setCategoryId(e.target.value)}>
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </div>
      <div className="form-row">
        <label>Title</label>
        <input value={title} onChange={(e) => setTitle(e.target.value)} />
      </div>
      <div className="form-row">
        <label>First post (markdown, optional)</label>
        <textarea value={body} onChange={(e) => setBody(e.target.value)} />
      </div>
      <div className="form-actions">
        <button className="btn" disabled={create.isPending || !title.trim()} onClick={() => create.mutate()}>
          Create
        </button>
        <button className="btn btn-secondary" onClick={() => setOpen(false)}>
          Cancel
        </button>
      </div>
      {message && <div className="state state-error">{message}</div>}
    </div>
  );
}
