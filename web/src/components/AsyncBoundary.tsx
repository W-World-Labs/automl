// The one loading / error / empty presenter — pages never hand-roll these
// three states (site-wide convention, see issue #7).

import type { ReactNode } from 'react';
import { errorCopy, isUnauthorized } from '../lib/errors';
import { loginHref } from '../api/client';

interface Props<T> {
  isLoading: boolean;
  error: unknown;
  data: T | undefined;
  /** Treat as empty when data exists but holds nothing (e.g. empty page). */
  isEmpty?: (data: T) => boolean;
  emptyText?: string;
  children: (data: T) => ReactNode;
}

export function AsyncBoundary<T>({
  isLoading,
  error,
  data,
  isEmpty,
  emptyText = 'Nothing here yet.',
  children,
}: Props<T>) {
  if (isLoading) return <div className="state state-loading">Loading…</div>;
  if (error) {
    if (isUnauthorized(error)) {
      return (
        <div className="state state-error">
          <p>{errorCopy(error)}</p>
          <a className="btn" href={loginHref()}>
            Sign in with W-ID
          </a>
        </div>
      );
    }
    return <div className="state state-error">{errorCopy(error)}</div>;
  }
  if (data === undefined || (isEmpty && isEmpty(data))) {
    return <div className="state state-empty">{emptyText}</div>;
  }
  return <>{children(data)}</>;
}
