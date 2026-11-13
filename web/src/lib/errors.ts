// Machine error code → human copy. `message` from the server is only a
// fallback — the code is the stable contract.

import { ApiError } from '../api/client';

const COPY: Record<string, string> = {
  UNAUTHORIZED: 'You need to sign in to do that.',
  FORBIDDEN: 'Your account does not have permission for this action.',
  NOT_FOUND: 'That resource does not exist (or was deleted).',
  CONFLICT: 'That change conflicts with the current state — reload and retry.',
  INVALID_INPUT: 'Some of the input is invalid.',
  PAYLOAD_TOO_LARGE: 'That upload is too large.',
  RATE_LIMITED: 'Too many requests — wait a moment and try again.',
  SIGNATURE_INVALID: 'The evidence signature could not be verified.',
  UNAVAILABLE: 'That service is temporarily unavailable.',
  NOT_IMPLEMENTED: 'This feature is not available on this deployment yet.',
  INTERNAL: 'Something went wrong on our side — please try again.',
};

export function errorCopy(err: unknown): string {
  if (err instanceof ApiError) {
    return COPY[err.code] ?? err.message ?? `Error (${err.code})`;
  }
  if (err instanceof Error) return err.message;
  return String(err);
}

export function errorCode(err: unknown): string | null {
  return err instanceof ApiError ? err.code : null;
}

export function isUnauthorized(err: unknown): boolean {
  return err instanceof ApiError && err.status === 401;
}
