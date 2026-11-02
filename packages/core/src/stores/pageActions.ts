/**
 * Global page-level action registry.
 *
 * Backs the "Create" button in the global PageHeader: each view
 * registers a handler while it is shown and clears its own when it
 * goes. The header
 * reads reactively, so the button is wired to the right action
 * for whatever view is currently mounted, without the previous
 * `defineExpose({ handleCreateThing })` + `currentViewComponent.value
 * ?.[methodName]?.()` string-typed plumbing.
 *
 * Why a store over the old defineExpose pattern:
 *  - No `any`-typed parent ref; the store is type-safe.
 *  - Survives `<KeepAlive>` lifecycles (paired with
 *    `onActivated` / `onDeactivated` in `usePageCreateAction`).
 *  - View can call `setCreateAction` from any descendent without
 *    a chain of `defineExpose`s.
 *
 * Visibility of the button is still gated on `route.meta
 * .createButtonText` so first-paint shows the right button label
 * before the view's `onMounted` fires. The handler simply no-ops
 * if a view forgets to register, rather than the button vanishing.
 *
 * One create at a time: a handler usually mints a row and then
 * navigates to it, and until it settles every further press joins the
 * one in flight instead of minting another. `creating` drives the
 * buttons' busy state. Every Create button reaches its handler through
 * `invokeCreate`, so this holds for all of them.
 */
import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'

export interface CreateAction {
  /** Invoked when the header Create button is clicked. */
  handler: () => void | Promise<void>
}

export const usePageActionsStore = defineStore('pageActions', () => {
  // Shallow, so the stored action is the object its view registered and
  // `clearCreateAction` can tell whose it is.
  const createAction = shallowRef<CreateAction | null>(null)
  const inFlight = ref<Promise<void> | null>(null)

  /** Registers the current view's action. Returns it, for `clearCreateAction`. */
  function setCreateAction(action: CreateAction | (() => void | Promise<void>)): CreateAction {
    const registered = typeof action === 'function' ? { handler: action } : action
    createAction.value = registered
    return registered
  }

  /**
   * Clears `registered` if it is still the current action, or whatever is
   * registered when called without one. A view leaving clears only its own:
   * the view arriving may have registered first (a cached list is activated
   * before the ticket page it returns from unmounts).
   */
  function clearCreateAction(registered?: CreateAction) {
    if (registered && createAction.value !== registered) return
    createAction.value = null
  }

  function invokeCreate(): Promise<void> {
    if (inFlight.value) return inFlight.value
    const action = createAction.value
    if (!action) return Promise.resolve()
    const run = (async () => {
      try {
        await action.handler()
      } finally {
        inFlight.value = null
      }
    })()
    inFlight.value = run
    return run
  }

  return {
    createAction,
    hasCreateAction: computed(() => createAction.value !== null),
    /** A create is running: its button shows busy and further presses join it. */
    creating: computed(() => inFlight.value !== null),
    setCreateAction,
    clearCreateAction,
    invokeCreate,
  }
})
