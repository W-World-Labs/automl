import { createHttpClient } from '@w/http-web';

/** App-local error contract: `API <status>` message + status/path fields (KEEP-LOCAL). */
export class ApiError extends Error {
  readonly status: number;
  readonly path: string;

  constructor(status: number, path: string) {
    super(`API ${status}`);
    this.status = status;
    this.path = path;
  }
}

export const apiUrl = (path: string) => `/api/v1${path}`;

const client = createHttpClient({
  baseUrl: '/api/v1',
  // Never inject Content-Type (the old wrapper added no headers of its own).
  contentType: 'none',
  // The old wrapper read the global `fetch` at call time; keep it late-bound.
  fetchImpl: (input, init) => fetch(input, init),
  // Preserve the local error contract exactly: body is ignored, message is `API <status>`.
  errorFactory: (info) => new ApiError(info.status, info.path),
});

export const api = client.fetchJSON;
