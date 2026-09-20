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
import { onMounted, onUnmounted, toValue, watchEffect, type MaybeRefOrGetter } from 'vue'

import { usePageActionsStore, type CreateAction } from '@nosdesk/core/stores/pageActions'

export interface PageCreateOptions {
  /**
   * FTL key for the header label when it depends on view state (the
   * People list's population); the route's first-paint label otherwise.
   * Followed reactively while the view is mounted.
   */
  labelKey?: MaybeRefOrGetter<string | undefined>
}

export function usePageCreateAction(
  action: CreateAction | (() => void | Promise<void>),
  options: PageCreateOptions = {},
): void {
  const store = usePageActionsStore()
  const handler = typeof action === 'function' ? action : action.handler

  let stop: (() => void) | null = null
  onMounted(() => {
    stop = watchEffect(() => store.setCreateAction({ handler, labelKey: toValue(options.labelKey) }))
  })
  onUnmounted(() => {
    stop?.()
    store.clearCreateAction()
  })
}
