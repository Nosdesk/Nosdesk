/**
 * This workspace's name, as the workspace switcher shows it: the name an
 * import keeps, and the one its confirmations ask to be typed. Falls back
 * to the branding's app name before the memberships load.
 */
import { computed, type ComputedRef } from 'vue'

import { useBrandingStore } from '@/stores/branding'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'

export function useWorkspaceName(): ComputedRef<string> {
  const workspaces = useMyWorkspacesStore()
  const branding = useBrandingStore()
  return computed(() => workspaces.activeWorkspace?.name || branding.appName || 'Nosdesk')
}
