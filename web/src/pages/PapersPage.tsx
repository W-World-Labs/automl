import { useInfiniteQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { api, type Page, type Paper } from '../api/client';
import { usePageTitle } from '../lib/title';
import { AsyncBoundary } from '../components/AsyncBoundary';

export function usePapersPages() {
  return useInfiniteQuery({
    queryKey: ['papers'],
    queryFn: ({ pageParam }) =>
      api<Page<Paper>>(
        `/api/v1/papers?limit=25${pageParam ? `&cursor=${encodeURIComponent(pageParam)}` : ''}`,
      ),
    initialPageParam: '',
    getNextPageParam: (last) => last.next_cursor ?? undefined,
  });
}

export function PapersPage() {
  usePageTitle('Papers');
  const q = usePapersPages();
  const papers = q.data?.pages.flatMap((p) => p.data);

  return (
    <section>
      <h1>Papers</h1>
      <AsyncBoundary
        isLoading={q.isLoading}
        error={q.error}
        data={papers}
        isEmpty={(items) => items.length === 0}
        emptyText="No papers published yet."
      >
        {(items) => (
          <>
            <ul className="item-list">
              {items.map((p) => (
                <li key={p.id}>
                  <Link className="item-title" to={`/papers/${p.id}`}>
                    {p.title}
                  </Link>
                  <div className="item-meta">
                    <span className={`badge badge-${p.review_status}`}>{p.review_status}</span>
                    <span className={`badge badge-${p.code_status}`}>{p.code_status}</span>
                    <span>{new Date(p.created_at).toLocaleDateString()}</span>
                  </div>
                </li>
              ))}
            </ul>
            {q.hasNextPage && (
              <div className="load-more">
                <button
                  className="btn btn-secondary"
                  disabled={q.isFetchingNextPage}
                  onClick={() => q.fetchNextPage()}
                >
                  {q.isFetchingNextPage ? 'Loading…' : 'Load more'}
                </button>
              </div>
            )}
          </>
        )}
      </AsyncBoundary>
    </section>
  );
}
