/**
 * The active workspace slug, as a plain module value.
 *
 * Decoupled on purpose (no imports): the router keeps it in sync on navigation
 * and the axios interceptor reads it to set the `X-Nosdesk-Workspace` header,
 * without either side importing the other (which would cycle through the store
 * and router). Only set in path mode; `null` in host mode means no header is
 * sent and the backend resolves the workspace from the Host, as today.
 */
const LAST_WORKSPACE_KEY = 'nosdesk:last-workspace';

let slug: string | null = null;

export function setActiveWorkspaceSlug(next: string | null): void {
  slug = next;
  // Remember the last workspace the user was on so the post-login landing can
  // return them there. Only persist a real slug; clearing on logout/switch
  // (null) must not erase the memory.
  if (next) {
    try {
      localStorage.setItem(LAST_WORKSPACE_KEY, next);
    } catch {
      // ignore (private mode / quota)
    }
  }
}

export function activeWorkspaceSlug(): string | null {
  return slug;
}

/** The last workspace slug this device was on, for the post-login landing. */
export function lastWorkspaceSlug(): string | null {
  try {
    return localStorage.getItem(LAST_WORKSPACE_KEY);
  } catch {
    return null;
  }
}
