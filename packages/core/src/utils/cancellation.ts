/**
 * Whether an error is a request the app chose not to finish, rather than a
 * failure: an axios `CanceledError` (an aborted signal, a request the request
 * gates refused, a response that no longer applies), a DOM `AbortError`, or the
 * legacy `REQUEST_CANCELLED` sentinel some services still throw.
 *
 * Duck-typed, so it holds across bundles and needs no axios import here: axios
 * marks its cancellations with `__CANCEL__` and code `ERR_CANCELED`.
 */
export function isCancellation(error: unknown): boolean {
  if (error === null || typeof error !== 'object') return false
  const e = error as { __CANCEL__?: unknown; code?: unknown; name?: unknown; message?: unknown }
  return (
    e.__CANCEL__ === true ||
    e.code === 'ERR_CANCELED' ||
    e.name === 'CanceledError' ||
    e.name === 'AbortError' ||
    e.message === 'REQUEST_CANCELLED'
  )
}
