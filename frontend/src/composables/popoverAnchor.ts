/**
 * Anchor and placement vocabulary for `<Popover>` and the surfaces built
 * on it (`ResponsiveMenu`, `HoverCard`, `BaseDropdown`). Positioning
 * itself is Reka UI's (floating-ui); these types are the consumer-facing
 * contract that predates it and did not need to change.
 */
export type PopoverPlacement =
  | 'top-start'
  | 'top-end'
  | 'top'
  | 'bottom-start'
  | 'bottom-end'
  | 'bottom'

export interface PopoverAnchorElement {
  type: 'element'
  /** Function (not ref) so consumers can return a `getBoundingClientRect`-able
   * source, usually `() => triggerRef.value`. */
  element: () => HTMLElement | null
}

/** A fixed viewport coordinate: right-click menus, click-anchored action
 * menus. Stale on scroll; consumers usually close the popover instead of
 * repositioning (`reactToScroll="close"`). */
export interface PopoverAnchorPoint {
  type: 'point'
  x: number
  y: number
}

export type PopoverAnchor = PopoverAnchorElement | PopoverAnchorPoint

/**
 * Turn an anchor + placement into what Reka's popper wants: a floating
 * reference (the element, or a virtual element at a viewport point) and
 * a side/align pair. Shared by `Popover` and `ResponsiveMenu`.
 */
export function floatingFrom(anchor: PopoverAnchor, placement: PopoverPlacement) {
  const side: 'top' | 'bottom' = placement.startsWith('top') ? 'top' : 'bottom'
  const align: 'start' | 'center' | 'end' = placement.endsWith('-start')
    ? 'start'
    : placement.endsWith('-end')
      ? 'end'
      : 'center'
  const element = anchor.type === 'element' ? anchor.element() : null
  const reference =
    anchor.type === 'element'
      ? (element ?? undefined)
      : { getBoundingClientRect: () => new DOMRect(anchor.x, anchor.y, 0, 0) }
  return { side, align, element, reference }
}

/** Anchor element for outside-dismiss filtering and focus restore. */
export function anchorElementOf(anchor: PopoverAnchor): HTMLElement | null {
  return anchor.type === 'element' ? anchor.element() : null
}

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

/**
 * Where focus goes back to when a surface closes from the keyboard: the
 * anchor itself when it is focusable, else the first focusable control
 * inside it (consumers often anchor to a wrapper for width matching).
 */
export function restoreFocusTo(anchor: PopoverAnchor): void {
  const el = anchorElementOf(anchor)
  if (!el) return
  const target = el.matches(FOCUSABLE) ? el : el.querySelector<HTMLElement>(FOCUSABLE)
  target?.focus?.()
}
