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
