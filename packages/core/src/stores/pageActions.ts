/**
 * Global page-level action registry.
 *
 * Backs the "Create" button in the global PageHeader: each view
 * registers a handler on mount, clears on unmount. The header
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
 */
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

export interface CreateAction {
  /** Invoked when the header Create button is clicked. */
  handler: () => void | Promise<void>
}

export const usePageActionsStore = defineStore('pageActions', () => {
  const createAction = ref<CreateAction | null>(null)

  function setCreateAction(action: CreateAction | (() => void | Promise<void>)) {
    createAction.value = typeof action === 'function' ? { handler: action } : action
  }

  function clearCreateAction() {
    createAction.value = null
  }

  async function invokeCreate(): Promise<void> {
    await createAction.value?.handler()
  }

  return {
    createAction,
    hasCreateAction: computed(() => createAction.value !== null),
    setCreateAction,
    clearCreateAction,
    invokeCreate,
  }
})
