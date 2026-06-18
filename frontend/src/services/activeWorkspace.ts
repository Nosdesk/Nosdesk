/**
 * The active workspace slug — the single source of truth for which workspace
 * the agent app is on in path mode.
 *
 * Deliberately depends only on `vue` (a leaf): the router keeps it in sync on
 * navigation, the axios interceptor reads it to set the `X-Nosdesk-Workspace`
 * header, and reactive consumers (the switcher via the myWorkspaces store) read
 * the ref, all without importing the router or store (which would cycle). Only
 * set in path mode; `null` in host mode means no header is sent and the backend
 * resolves the workspace from the Host, as today.
 */
import { readonly, ref, type Ref } from 'vue';

const LAST_WORKSPACE_KEY = 'nosdesk:last-workspace';

const slug = ref<string | null>(null);

export function setActiveWorkspaceSlug(next: string | null): void {
  slug.value = next;
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

/** Non-reactive read, for the axios interceptor / sync engine (outside Vue). */
export function activeWorkspaceSlug(): string | null {
  return slug.value;
}

/** Reactive read, for UI (the workspace switcher's active-workspace highlight). */
export const activeWorkspaceSlugRef: Readonly<Ref<string | null>> = readonly(slug);

/** The last workspace slug this device was on, for the post-login landing. */
export function lastWorkspaceSlug(): string | null {
  try {
    return localStorage.getItem(LAST_WORKSPACE_KEY);
  } catch {
    return null;
  }
}
