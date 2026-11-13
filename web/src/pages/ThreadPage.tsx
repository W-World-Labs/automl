import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useParams } from 'react-router-dom';
import { api, type Page, type Post, type Thread } from '../api/client';
import { AsyncBoundary } from '../components/AsyncBoundary';
import { errorCopy } from '../lib/errors';
import { renderMarkdown } from '../lib/markdown';
import { useSession } from '../lib/session';
import { usePageTitle } from '../lib/title';

export function ThreadPage() {
  const { id } = useParams<{ id: string }>();
  const thread = useQuery({
    queryKey: ['forum', 'thread', id],
    queryFn: () => api<Thread>(`/api/v1/forum/threads/${id}`),
  });

  return (
    <AsyncBoundary isLoading={thread.isLoading} error={thread.error} data={thread.data}>
      {(t) => (
        <section>
          <ThreadTitle title={t.title} />
          <h1>{t.title}</h1>
          <Posts threadId={t.id} />
        </section>
      )}
    </AsyncBoundary>
  );
}

function Posts({ threadId }: { threadId: string }) {
  const qc = useQueryClient();
  const session = useSession();
  const posts = useQuery({
    queryKey: ['forum', 'posts', threadId],
    queryFn: () => api<Page<Post>>(`/api/v1/forum/threads/${threadId}/posts?limit=100`),
  });
  const [body, setBody] = useState('');
  const [message, setMessage] = useState<string | null>(null);

  const reply = useMutation({
    mutationFn: () =>
      api<Post>(`/api/v1/forum/threads/${threadId}/posts`, {
        method: 'POST',
        body: JSON.stringify({ body }),
      }),
    onSuccess: () => {
      setBody('');
      setMessage(null);
      qc.invalidateQueries({ queryKey: ['forum', 'posts', threadId] });
    },
    onError: (err) => setMessage(errorCopy(err)),
  });

  const remove = useMutation({
    mutationFn: (postId: string) => api(`/api/v1/forum/posts/${postId}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['forum', 'posts', threadId] }),
    onError: (err) => setMessage(errorCopy(err)),
  });

  return (
    <>
      <AsyncBoundary
        isLoading={posts.isLoading}
        error={posts.error}
        data={posts.data?.data}
        isEmpty={(p) => p.length === 0}
        emptyText="No posts in this thread."
      >
        {(items) => (
          <ul className="item-list">
            {items.map((p) => (
              <li key={p.id}>
                <div
                  className="markdown"
                  // Sanitized in renderMarkdown (DOMPurify).
                  dangerouslySetInnerHTML={{ __html: renderMarkdown(p.body) }}
                />
                <div className="item-meta">
                  <span>by {p.created_by.slice(0, 8)}</span>
                  <span>{new Date(p.created_at).toLocaleString()}</span>
                  {session.data?.authenticated && (
                    <button className="linklike" onClick={() => remove.mutate(p.id)}>
                      delete
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </AsyncBoundary>

      <div className="panel">
        <h3>Reply</h3>
        <textarea
          value={body}
          onChange={(e) => setBody(e.target.value)}
          placeholder="Markdown supported"
        />
        <div className="form-actions">
          <button className="btn" disabled={reply.isPending || !body.trim()} onClick={() => reply.mutate()}>
            Post reply
          </button>
        </div>
        {message && <div className="state state-error">{message}</div>}
      </div>
    </>
  );
}

function ThreadTitle({ title }: { title: string }) {
  usePageTitle(title);
  return null;
}
