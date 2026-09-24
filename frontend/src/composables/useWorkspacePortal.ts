/**
 * The workspace's public portal: its address and which public surfaces are on.
 *
 * Links to the portal must use this, not `window.location.origin`: on hosted
 * the agent app runs on one shared origin, so `/submit-ticket` or `/docs` on
 * it isn't the workspace's portal.
 */
import { computed } from 'vue';
import { useQuery } from '@pinia/colada';
import { getWorkspacePortal } from '@nosdesk/core/services/workspacePortalService';
import { workspaceReadyRef } from '@/services/activeWorkspace';

export function useWorkspacePortal() {
  const query = useQuery({
    key: ['workspace-portal'],
    query: getWorkspacePortal,
    enabled: () => workspaceReadyRef.value,
  });
  const portal = computed(() => query.data.value ?? null);
  /** Full URL of a portal path, e.g. `portalUrl('/submit-ticket')`. */
  function portalUrl(path: string): string {
    const origin = portal.value?.portal_url ?? window.location.origin;
    return `${origin.replace(/\/$/, '')}${path}`;
  }
  return { portal, portalUrl };
}
