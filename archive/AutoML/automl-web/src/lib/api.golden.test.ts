// GOLDEN regression spec for the api() wrapper contract.
//
// Written against the ORIGINAL hand-rolled wrapper FIRST (and verified green),
// then required to pass unchanged after the @w/http-web adoption. Pins the
// user-visible / wire contract, normalised through `new Request(input, init)`
// so it asserts the exact bytes the browser would send — independent of the
// internal (url, init) call shape.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, api, apiUrl } from './api';

type FetchArgs = [input: RequestInfo | URL, init?: RequestInit];

const captureFetch = (response: () => Response) => {
  const calls: FetchArgs[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (...args: FetchArgs) => {
      calls.push(args);
      return response();
    }),
  );
  return calls;
};

/** Fixed base so same-origin relative URLs normalise identically pre/post swap. */
const BASE = 'http://golden.local';

/** Normalise a captured fetch call to the actual wire request. */
const wireOf = (args: FetchArgs) => {
  const req = new Request(new URL(String(args[0]), BASE), args[1]);
  const headers: [string, string][] = [];
  req.headers.forEach((v, k) => headers.push([k, v]));
  headers.sort((a, b) => a[0].localeCompare(b[0]));
  return { rawUrl: String(args[0]), url: req.url, method: req.method, headers, body: req.body };
};

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('api wire-contract goldens', () => {
  it('apiUrl produces exact path bytes', () => {
    expect(apiUrl('/papers?limit=10')).toBe('/api/v1/papers?limit=10');
    expect(apiUrl('/silicon-scholars/x%20y')).toBe('/api/v1/silicon-scholars/x%20y');
  });

  it('bare GET sends exactly GET /api/v1<path> with zero headers and no body', async () => {
    const calls = captureFetch(() => new Response(JSON.stringify({ status: 'ok' })));

    await expect(api<{ status: string }>('/health')).resolves.toEqual({ status: 'ok' });

    expect(calls).toHaveLength(1);
    const wire = wireOf(calls[0]);
    expect(wire.rawUrl).toBe('/api/v1/health'); // exact URL string handed to fetch
    expect(wire.url).toBe(`${BASE}/api/v1/health`);
    expect(wire.method).toBe('GET');
    expect(wire.headers).toEqual([]); // no injected Content-Type / Accept / auth
    expect(wire.body).toBeNull();
  });

  it('success body is parsed as JSON and returned verbatim', async () => {
    const payload = { papers: [{ id: 'p1', title: 'T' }], nested: { n: 1, b: true, z: null } };
    captureFetch(() => new Response(JSON.stringify(payload)));

    await expect(api('/papers?limit=50')).resolves.toEqual(payload);
  });

  it('invalid JSON on a 2xx rejects with a SyntaxError', async () => {
    captureFetch(() => new Response('not-json', { status: 200 }));

    await expect(api('/health')).rejects.toThrow(SyntaxError);
  });

  it('non-2xx throws local ApiError: exact message/status/path, body ignored', async () => {
    captureFetch(() => new Response('missing', { status: 404, statusText: 'Not Found' }));

    const err = await api('/missing').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toBeInstanceOf(Error);
    const apiErr = err as ApiError;
    expect(apiErr.message).toBe('API 404'); // exact bytes, never the response body
    expect(apiErr.status).toBe(404);
    expect(apiErr.path).toBe('/missing');
  });

  it('non-2xx JSON {error}/{message} bodies do NOT leak into the error message', async () => {
    captureFetch(
      () =>
        new Response(JSON.stringify({ error: 'boom', message: 'nope', detail: 'no' }), {
          status: 500,
          statusText: 'Internal Server Error',
        }),
    );

    const err = await api('/stats').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).message).toBe('API 500');
    expect((err as ApiError).status).toBe(500);
    expect((err as ApiError).path).toBe('/stats');
  });

  it('401 throws the same local ApiError (no redirect / hang / special-casing)', async () => {
    captureFetch(() => new Response('', { status: 401, statusText: 'Unauthorized' }));

    const err = await api('/skills').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).message).toBe('API 401');
    expect((err as ApiError).status).toBe(401);
    expect((err as ApiError).path).toBe('/skills');
  });

  it('per-call init passes through byte-for-byte: method, caller headers, body; nothing injected', async () => {
    const calls = captureFetch(() => new Response(JSON.stringify({ ok: true })));

    await api('/skills/run', {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain', 'X-Trace': 'abc' },
      body: 'raw-payload',
    });

    const wire = wireOf(calls[0]);
    expect(wire.rawUrl).toBe('/api/v1/skills/run');
    expect(wire.method).toBe('POST');
    expect(wire.headers).toEqual([
      ['content-type', 'text/plain'], // caller's value, NOT application/json
      ['x-trace', 'abc'],
    ]);
    expect(wire.body).not.toBeNull();
    const sentBody = await new Request(new URL(String(calls[0][0]), BASE), calls[0][1]).text();
    expect(sentBody).toBe('raw-payload'); // body bytes pass through untouched
  });

  it('GET never gains a Content-Type header', async () => {
    const calls = captureFetch(() => new Response(JSON.stringify({})));

    await api('/papers/p1');

    expect(wireOf(calls[0]).headers).toEqual([]);
  });

  it('late-binds the global fetch (stub installed after module import is honoured)', async () => {
    // The original wrapper reads `fetch` from the global at call time; the
    // adoption must keep that (vi.stubGlobal in every test relies on it).
    const calls = captureFetch(() => new Response(JSON.stringify({ status: 'ok' })));
    await api('/health');
    expect(calls).toHaveLength(1);
  });
});
