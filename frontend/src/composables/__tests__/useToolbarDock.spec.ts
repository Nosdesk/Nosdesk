import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import { useToolbarDock } from '@/composables/useToolbarDock'

function setup() {
  const enabled = ref(true)
  const mobile = ref(true)
  const canEdit = ref(true)
  const focused = ref(false)
  const menuOpen = ref(false)
  const dock = useToolbarDock({ enabled, mobile, canEdit, focused, menuOpen })
  return { enabled, mobile, canEdit, focused, menuOpen, dock }
}

describe('useToolbarDock', () => {
  it('docks only while the editor has focus', () => {
    const s = setup()
    expect(s.dock.value).toBe(false)
    s.focused.value = true
    expect(s.dock.value).toBe(true)
    s.focused.value = false
    expect(s.dock.value).toBe(false)
  })

  it('stays docked while a toolbar menu holds focus', () => {
    const s = setup()
    s.focused.value = true
    s.focused.value = false
    s.menuOpen.value = true
    expect(s.dock.value).toBe(true)
    s.menuOpen.value = false
    expect(s.dock.value).toBe(false)
  })

  it('folds when a menu is dismissed without a choice', () => {
    // Backdrop tap or Escape: nothing refocuses the editor, so the dock
    // goes with the keyboard until the next tap in the editor. Expected.
    const s = setup()
    s.menuOpen.value = true
    expect(s.dock.value).toBe(true)
    s.menuOpen.value = false
    expect(s.dock.value).toBe(false)
  })

  it('never docks on desktop, for a host that did not opt in, or for a reader', () => {
    const s = setup()
    s.focused.value = true
    s.mobile.value = false
    expect(s.dock.value).toBe(false)
    s.mobile.value = true
    s.enabled.value = false
    expect(s.dock.value).toBe(false)
    s.enabled.value = true
    s.canEdit.value = false
    expect(s.dock.value).toBe(false)
  })
})
