import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, api, apiUrl } from './api';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('api', () => {
  it('prefixes Auto.ml API routes', () => {
    expect(apiUrl('/health')).toBe('/api/v1/health');
  });

  it('returns JSON for successful API responses', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ status: 'ok' }))));

    await expect(api<{ status: string }>('/health')).resolves.toEqual({ status: 'ok' });
    // Wire-equivalent pin (was `toHaveBeenCalledWith(url, undefined)`, an
    // internal call-shape detail): same URL bytes, and the effective request
    // is still a bare GET with zero injected headers — identical HTTP bytes.
    expect(fetch).toHaveBeenCalledTimes(1);
    const [calledUrl, calledInit] = vi.mocked(fetch).mock.calls[0];
    expect(calledUrl).toBe('/api/v1/health');
    const req = new Request(new URL(String(calledUrl), 'http://golden.local'), calledInit);
    expect(req.method).toBe('GET');
    expect([...req.headers.entries()]).toEqual([]);
    expect(req.body).toBeNull();
  });

  it('throws ApiError with route context on failed responses', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('missing', { status: 404 })));

    await expect(api('/missing')).rejects.toMatchObject(new ApiError(404, '/missing'));
  });
});
