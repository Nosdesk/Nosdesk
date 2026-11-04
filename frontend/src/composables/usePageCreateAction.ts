/**
 * Lifecycle-correct registration helper for the global Create
 * button's per-view handler.
 *
 * The view registers while it is shown and releases its own
 * registration when it goes. A KeepAlive-cached view (the tickets
 * and assets lists, for swipe-back) is activated rather than
 * mounted when you return to it, and the view you came from
 * unmounts after that, so registration follows activation too and
 * a release never clears another view's action.
 */
import { onActivated, onDeactivated, onMounted, onUnmounted } from 'vue'

import { usePageActionsStore, type CreateAction } from '@nosdesk/core/stores/pageActions'

export function usePageCreateAction(
  action: CreateAction | (() => void | Promise<void>),
): void {
  const store = usePageActionsStore()
  let registered: CreateAction | undefined

  const register = () => {
    registered = store.setCreateAction(action)
  }
  const release = () => {
    if (registered) store.clearCreateAction(registered)
    registered = undefined
  }

  onMounted(register)
  onActivated(register)
  onDeactivated(release)
  onUnmounted(release)
}
