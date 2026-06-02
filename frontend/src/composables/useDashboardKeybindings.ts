/**
 * Keyboard shortcuts for the dashboard. Registered once at the
 * `DashboardView` level; tears down on unmount.
 *
 * Shortcuts:
 *   - 1..=7    jump to the matching anchor in `SECTIONS`
 *   - e        enter edit mode (no-op if already editing)
 *   - r        refresh non-live data
 *   - esc      discard the in-flight edit session (when dirty)
 *
 * Editor-friendly: every shortcut bails out when the active element
 * is an input / textarea / contenteditable / search field, so the
 * dashboard doesn't intercept typing.
 */
import { onBeforeUnmount, onMounted } from 'vue'
import type { AnchorScroll } from './useAnchorScroll'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import { SECTIONS } from '@/views/dashboard/sections'

export interface DashboardKeybindingsOptions {
  anchorScroll: AnchorScroll
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
    if (e.metaKey || e.ctrlKey || e.altKey) return
    if (isTypingTarget(e.target)) return

    // Number keys 1..=7: jump to the matching section anchor. Uses
    // the SECTIONS list order, so renumbering there renumbers the
    // shortcuts here too.
    if (/^[1-9]$/.test(e.key)) {
      const idx = parseInt(e.key, 10) - 1
      const section = SECTIONS[idx]
      if (section) {
        e.preventDefault()
        options.anchorScroll.scrollTo(section.id)
      }
      return
    }

    switch (e.key.toLowerCase()) {
      case 'e':
        // Enter edit mode unless already inside one; the store
        // ignores re-entry, but we still avoid the event noise.
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
