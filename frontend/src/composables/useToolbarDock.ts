import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'

/**
 * Whether an editor's formatting toolbar should leave its inline position
 * and dock on the on-screen keyboard.
 *
 * Docked only when the host opted in, the viewport is a phone, the user can
 * edit, and either the editor has focus or one of the toolbar's own menus is
 * open. The menu case matters: a bottom-sheet menu takes focus from the
 * editor while it is open, and without it the toolbar would fold away under
 * the user's thumb between picking a text style and the editor refocusing.
 */
export function useToolbarDock(opts: {
  enabled: MaybeRefOrGetter<boolean>
  mobile: MaybeRefOrGetter<boolean>
  canEdit: MaybeRefOrGetter<boolean>
  focused: MaybeRefOrGetter<boolean>
  menuOpen: MaybeRefOrGetter<boolean>
}): ComputedRef<boolean> {
  return computed(
    () =>
      toValue(opts.enabled) &&
      toValue(opts.mobile) &&
      toValue(opts.canEdit) &&
      (toValue(opts.focused) || toValue(opts.menuOpen)),
  )
}
