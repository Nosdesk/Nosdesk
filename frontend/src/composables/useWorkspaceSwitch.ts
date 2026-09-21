/**
 * Workspace switching (Model C, increment 3, stage 5).
 *
 * In `path` mode (single origin) switching is an in-app teardown + re-hydrate,
 * done by the workspace guard when the URL's slug changes (see
 * `router/workspaceRouting.ts`): reset all workspace-scoped state, publish the
 * new slug, load the new workspace's branding and plugins, and let the hydrate
 * guard re-establish the sync pool + SSE (its IDB handle was closed, so it
 * bootstraps fresh). `isSwitchingWorkspace` covers the gap so no stale or empty
 * data flashes between the two workspaces.
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
      // Let the shell mask the content (and unmount the current view) before
      // the guard tears its data out from under it.
      await nextTick();
      // The workspace guard owns the switch itself: it sees the slug change,
      // resets the previous workspace's state, publishes the new slug and
      // loads what the new workspace needs. The same thing happens for a
      // typed URL or a deep link, so this composable only adds the mask.
      await router.push(destinationPath ?? `/${entry.slug}`);
    } catch (e) {
      logger.error('Workspace switch failed', e);
    } finally {
      isSwitchingWorkspace.value = false;
    }
  }

  return { isSwitchingWorkspace, switchWorkspace };
}
