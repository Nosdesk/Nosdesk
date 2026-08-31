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
import { computed, readonly, ref, type Ref } from 'vue';
import { addRequestHeaderProvider } from '@nosdesk/core/transport';
import {
  getControlPlaneUrl,
  getWorkspaceRouting,
  instanceConfigResolvedRef,
} from '@nosdesk/core/services/instanceConfig';

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

/**
 * The control-plane Seats deep-link for the active workspace, so a hosted admin
 * lands on THIS instance's seats rather than the generic instances list. The
 * control plane resolves `?workspace=<slug>` to the instance (it owns the
 * slug->instance mapping). Returns `''` when the control-plane URL is unknown
 * (self-hosted, or the config has not resolved), so callers can skip the link.
 */
export function controlPlaneSeatsUrl(): string {
  const cp = getControlPlaneUrl();
  if (!cp) return '';
  const s = slug.value;
  return s ? `${cp}/instances?workspace=${encodeURIComponent(s)}` : `${cp}/instances`;
}

/** Reactive read, for UI (the workspace switcher's active-workspace highlight). */
export const activeWorkspaceSlugRef: Readonly<Ref<string | null>> = readonly(slug);

/**
 * The selection header to attach to a request, or `{}` when none. One place for
 * the header name + slug read, shared by the axios interceptor and the sync
 * engine's raw `fetch` calls (which the interceptor doesn't see).
 */
export function workspaceHeaders(): Record<string, string> {
  return slug.value ? { 'X-Nosdesk-Workspace': slug.value } : {};
}

// Publish the selection header through the core transport seam so every consumer
// (the web apiConfig interceptor and the mobile interceptor, whose bootstrap
// clears apiConfig) attaches it. Registered at module load: the router's
// workspace guard imports this module before any request fires, so the workspace
// header is available early — ahead of the diagnostics provider apiConfig adds.
addRequestHeaderProvider(workspaceHeaders);

/**
 * Reactive gate for whether a workspace-scoped request may fire yet. Workspace-
 * scoped Colada queries read it in `enabled` so nothing fires header-less in the
 * first-login window (auth flips true before the slug is set), which would fail
 * closed as `NoWorkspaceSelected`. Not ready until `/api/config` resolves (the
 * 'host' default is not an answer); host mode is ready once resolved (backend
 * resolves from Host); path mode is ready once a slug is selected.
 *
 * `getWorkspaceRouting()` is read non-reactively on purpose: routing is settled
 * before `instanceConfigResolvedRef` flips, so gating on the latter is enough.
 */
export const workspaceReadyRef = computed<boolean>(() => {
  if (!instanceConfigResolvedRef.value) return false;
  if (getWorkspaceRouting() !== 'path') return true;
  return slug.value !== null;
});

/** Non-reactive read of the workspace-ready gate, for use outside a template. */
export function workspaceReady(): boolean {
  return workspaceReadyRef.value;
}

/** The last workspace slug this device was on, for the post-login landing. */
export function lastWorkspaceSlug(): string | null {
  try {
    return localStorage.getItem(LAST_WORKSPACE_KEY);
  } catch {
    return null;
  }
}
