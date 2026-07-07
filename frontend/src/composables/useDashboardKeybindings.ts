/**
 * Keyboard shortcuts for the dashboard. Registered once at the
 * `DashboardView` level; tears down on unmount.
 *
 * Shortcuts:
 *   - e            enter edit mode (no-op if already editing)
 *   - r            refresh non-live data
 *   - esc          discard the in-flight edit session (when dirty)
 *   - cmd/ctrl-z   undo the last edit-session change
 *   - cmd/ctrl-shift-z  redo
 *
 * Anchor jumps (1..=7) are wired alongside the section anchors that
 * v1.1 reintroduces; absent in v1.
 *
 * Editor-friendly: every shortcut bails out when the active element
 * is an input / textarea / contenteditable / search field, so the
 * dashboard doesn't intercept typing.
 */
import { onBeforeUnmount, onMounted } from 'vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'

export interface DashboardKeybindingsOptions {
  onEditMode: () => void
  onRefresh: () => void
}

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true
  if (target.isContentEditable) return true
  return false
}

export function useDashboardKeybindings(options: DashboardKeybindingsOptions) {
  const store = useDashboardLayoutStore()

  function onKeyDown(e: KeyboardEvent) {
    // Undo / redo, scoped to the edit session (decision 17). Checked
    // before the modifier bail below. Typing targets keep their own
    // native undo.
    if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 'z') {
      if (!store.editMode || isTypingTarget(e.target)) return
      e.preventDefault()
      if (e.shiftKey) store.redo()
      else store.undo()
      return
    }

    if (e.metaKey || e.ctrlKey || e.altKey) return
    if (isTypingTarget(e.target)) return

    switch (e.key.toLowerCase()) {
      case 'e':
        if (!store.editMode) {
          e.preventDefault()
          options.onEditMode()
        }
        break
      case 'r':
        e.preventDefault()
        options.onRefresh()
        break
      case 'escape':
        if (store.editMode && store.isDirty) {
          e.preventDefault()
          store.discard()
        }
        break
    }
  }

  onMounted(() => {
    document.addEventListener('keydown', onKeyDown)
  })
  onBeforeUnmount(() => {
    document.removeEventListener('keydown', onKeyDown)
  })
}
