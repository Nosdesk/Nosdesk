/**
 * Guard against losing unsaved form edits.
 *
 * Wires two exits:
 *  - In-app route navigation: `onBeforeRouteLeave` defers the
 *    navigation while `isDirty` is true and surfaces a styled
 *    ConfirmModal (the caller renders it, bound to the returned
 *    state/handlers). This mirrors the DashboardView leave-guard and
 *    deliberately avoids the native `window.confirm` removed in the
 *    confirm-modal sweep.
 *  - Tab close / reload: a `beforeunload` listener triggers the
 *    browser's own "leave site?" prompt while dirty.
 *
 * Usage:
 *   const { showLeaveConfirm, confirmLeave, cancelLeave } =
 *     useUnsavedChanges(dirty)
 *   // in template:
 *   <ConfirmModal :show="showLeaveConfirm" ... @confirm="confirmLeave"
 *     @close="cancelLeave" />
 *
 * `onConfirm` runs when the user confirms leaving (e.g. to reset a
 * working-copy store) before the navigation proceeds.
 */
import { ref, computed, onMounted, onBeforeUnmount, type Ref } from 'vue'
import { onBeforeRouteLeave, type NavigationGuardNext } from 'vue-router'

export function useUnsavedChanges(
  isDirty: Ref<boolean>,
  options?: { onConfirm?: () => void },
) {
  const pendingLeave = ref<NavigationGuardNext | null>(null)
  const showLeaveConfirm = computed(() => pendingLeave.value !== null)

  onBeforeRouteLeave((_to, _from, next) => {
    if (!isDirty.value) {
      next()
      return
    }
    pendingLeave.value = next
  })

  function confirmLeave() {
    options?.onConfirm?.()
    const next = pendingLeave.value
    pendingLeave.value = null
    next?.()
  }

  function cancelLeave() {
    const next = pendingLeave.value
    pendingLeave.value = null
    next?.(false)
  }

  function handleBeforeUnload(e: BeforeUnloadEvent) {
    if (!isDirty.value) return
    e.preventDefault()
    // Legacy browsers need returnValue set to trigger the prompt.
    e.returnValue = ''
  }

  onMounted(() => window.addEventListener('beforeunload', handleBeforeUnload))
  onBeforeUnmount(() => window.removeEventListener('beforeunload', handleBeforeUnload))

  return { showLeaveConfirm, confirmLeave, cancelLeave }
}
