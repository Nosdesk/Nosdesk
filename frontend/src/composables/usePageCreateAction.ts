/**
 * Lifecycle-correct registration helper for the global Create
 * button's per-view handler.
 *
 * Plain `onMounted`/`onUnmounted` is enough now that list views
 * are no longer KeepAlive-cached (see the comment in
 * `App.vue`'s KeepAlive block). The cached-view branch with
 * `onActivated`/`onDeactivated` was load-bearing only because
 * KeepAlive doesn't unmount; with views unmounting on nav-away
 * the registration follows the natural component lifecycle.
 *
 * `TicketView` is still KeepAlive-cached but doesn't register a
 * page-action that conflicts with sibling views, so this
 * simplification doesn't regress it.
 */
import { onMounted, onUnmounted } from 'vue'

import { usePageActionsStore, type CreateAction } from '@nosdesk/core/stores/pageActions'

export function usePageCreateAction(
  action: CreateAction | (() => void | Promise<void>),
): void {
  const store = usePageActionsStore()

  onMounted(() => store.setCreateAction(action))
  onUnmounted(() => store.clearCreateAction())
}
