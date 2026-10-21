/**
 * The active workspace's trial reminder for the signed-in owner, or `null`.
 *
 * Billing arrives only on a hosted owner's workspace row, so `null` covers
 * self-hosted, admins and agents, a paid plan, and the moment before the row
 * loads alike. The day count moves when the tab comes back into view, which
 * is when a reminder left open overnight would otherwise be a day stale.
 */
import { computed, onScopeDispose, ref } from 'vue'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import { isHostedDeploymentRef } from '@nosdesk/core/services/instanceConfig'
import { useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { useAuthStore } from '@/stores/auth'
import { trialNotice } from '@/utils/trialNotice'

export function useTrialNotice() {
  const workspaces = useMyWorkspacesStore()
  const auth = useAuthStore()
  const dateStore = useDateStore()

  const now = ref(new Date())
  const refresh = () => {
    if (document.visibilityState === 'visible') now.value = new Date()
  }
  document.addEventListener('visibilitychange', refresh)
  onScopeDispose(() => document.removeEventListener('visibilitychange', refresh))

  const notice = computed(() => {
    const ws = workspaces.activeWorkspace
    if (!isHostedDeploymentRef.value || !ws || ws.role !== 'owner') return null
    return trialNotice(ws.billing, now.value, dateStore.locale)
  })

  /** The dismissal key's owner: workspace and person. Empty until both load. */
  const scope = computed(() => ({
    workspace: workspaces.activeWorkspace?.workspace_uuid ?? '',
    user: auth.user?.uuid ?? '',
  }))

  return { notice, scope }
}
