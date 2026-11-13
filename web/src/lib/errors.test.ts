import { describe, expect, it } from 'vitest';
import { ApiError } from '../api/client';
import { errorCopy, isUnauthorized } from './errors';

describe('errorCopy', () => {
  it('maps known codes to copy, ignoring server message', () => {
    const err = new ApiError('RATE_LIMITED', 'rate limit exceeded; retry after 3s', 429);
    expect(errorCopy(err)).toMatch(/wait a moment/i);
  });

  it('falls back to server message for unknown codes', () => {
    const err = new ApiError('SOMETHING_NEW', 'a brand new failure', 400);
    expect(errorCopy(err)).toBe('a brand new failure');
  });

  it('handles plain errors', () => {
    expect(errorCopy(new Error('boom'))).toBe('boom');
  });

  it('detects 401 for login redirects', () => {
    expect(isUnauthorized(new ApiError('UNAUTHORIZED', 'x', 401))).toBe(true);
    expect(isUnauthorized(new ApiError('FORBIDDEN', 'x', 403))).toBe(false);
  });
});
