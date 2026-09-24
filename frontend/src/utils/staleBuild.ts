/**
 * Recovery for a tab left open across a deploy. Each build renames its code
 * chunks, so an old tab's next lazy import (a route, a dialog) asks for a file
 * the server no longer has. Loading the new build fixes it; without this the
 * navigation failed onto the generic error page.
 *
 * One reload per window: if the fresh page still can't load a chunk, the file
 * is really missing and reloading again would loop.
 */
const RELOAD_KEY = 'nosdesk:stale-build-reload-at';
const RELOAD_WINDOW_MS = 10_000;

const CHUNK_ERROR =
  /Failed to fetch dynamically imported module|Importing a module script failed|error loading dynamically imported module|Unable to preload CSS/i;

/** True for the error a lazy import throws when its chunk is gone. */
export function isStaleChunkError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error ?? '');
  return CHUNK_ERROR.test(message);
}

/**
 * Reload into `path` (default: the current page) to pick up the new build.
 * Returns false, doing nothing, if a reload already happened moments ago.
 */
export function reloadForNewBuild(path?: string): boolean {
  const now = Date.now();
  try {
    const last = Number(sessionStorage.getItem(RELOAD_KEY) ?? 0);
    if (now - last < RELOAD_WINDOW_MS) return false;
    sessionStorage.setItem(RELOAD_KEY, String(now));
  } catch {
    // No storage (private mode): reload anyway; the browser keeps us safe
    // from a tight loop because each reload is a full navigation.
  }
  if (path) window.location.assign(path);
  else window.location.reload();
  return true;
}
