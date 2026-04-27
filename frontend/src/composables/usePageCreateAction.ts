/**
 * Lifecycle-correct registration helper for the global Create
 * button's per-view handler. Wraps `usePageActionsStore` so
 * every view doesn't have to repeat the four lifecycle hooks.
 *
 * Handles both ordinary mount/unmount and `<KeepAlive>` activate
 * /deactivate, so cached views correctly re-register their
 * handler on re-entry without leaking the previous one.
 */
import { onActivated, onDeactivated, onMounted, onUnmounted } from 'vue'

import { usePageActionsStore, type CreateAction } from '@/stores/pageActions'

export function usePageCreateAction(
  action: CreateAction | (() => void | Promise<void>),
): void {
  const store = usePageActionsStore()
  const register = () => store.setCreateAction(action)
  const unregister = () => store.clearCreateAction()

  onMounted(register)
  onActivated(register)
  onDeactivated(unregister)
  onUnmounted(unregister)
}
