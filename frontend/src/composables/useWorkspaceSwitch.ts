/**
 * Workspace switching (Model C, increment 3, stage 5).
 *
 * In `path` mode (single origin) switching is an in-app teardown + re-hydrate:
 * reset all workspace-scoped state, navigate to the new slug, and let the
 * router's hydrate guard re-establish the sync pool + SSE for the new workspace
 * (its IDB handle was just closed, so it bootstraps fresh), then re-fetch the
 * new workspace's branding. `isSwitchingWorkspace` covers the gap so no stale or
 * empty data flashes between the two workspaces.
 *
 * In `host` mode each workspace is a separate origin, so switching stays a full
 * navigation to the other subdomain (today's behaviour).
 */
import { ref, nextTick } from 'vue';
import { useRouter } from 'vue-router';
import { getWorkspaceRouting } from '@nosdesk/core/services/instanceConfig';
import { navigateToWorkspace } from '@/utils/workspaceNavigation';
import { logger } from '@nosdesk/core/utils/logger';
import type { MyWorkspaceEntry } from '@nosdesk/core/types/workspace';

/** Shared so the app shell can mask the content while a switch is in flight. */
const isSwitchingWorkspace = ref(false);

export function useWorkspaceSwitch() {
  const router = useRouter();

  /**
   * Switch to `entry`. Lands on the workspace home by default, or on
   * `destinationPath` (already slug-prefixed, e.g. a ticket deep link
   * `/acme/tickets/8`) when the caller wants to arrive somewhere specific.
   */
  async function switchWorkspace(
    entry: MyWorkspaceEntry,
    destinationPath?: string,
  ): Promise<void> {
    if (getWorkspaceRouting() !== 'path') {
      navigateToWorkspace(entry);
      return;
    }
    if (isSwitchingWorkspace.value) return;
    isSwitchingWorkspace.value = true;
    try {
      // Let the shell mask the content (and unmount the current view) before we
      // tear its data out from under it.
      await nextTick();
      const { resetWorkspaceScopedState } = await import('@/stores/workspaceReset');
      await resetWorkspaceScopedState();
      // Navigate to the target (workspace home, or a specific deep-link path).
      // The prefix guard sets the active slug and the hydrate guard re-bootstraps
      // + re-attaches SSE for it (the pool's IDB handle was closed by the reset,
      // so it starts clean).
      await router.push(destinationPath ?? `/${entry.slug}`);
      const { useBrandingStore } = await import('@/stores/branding');
      await useBrandingStore().loadBranding();
    } catch (e) {
      logger.error('Workspace switch failed', e);
    } finally {
      isSwitchingWorkspace.value = false;
    }
  }

  return { isSwitchingWorkspace, switchWorkspace };
}
