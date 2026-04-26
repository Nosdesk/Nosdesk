/**
 * Popover positioning math, viewport-aware. The composable owns
 * "where on screen does the floating thing go?" and nothing else;
 * dismissal, focus, and DOM mounting belong to the consumer
 * (typically the `<Popover>` primitive).
 *
 * Two anchor kinds:
 *   - `element`: anchor to a DOM element (dropdowns, comboboxes,
 *     tooltips). Tracked across scroll if the consumer asks.
 *   - `point`: anchor to a fixed viewport coordinate (right-click
 *     menus, click-anchored action menus). Stale on scroll;
 *     consumers usually close the popover instead of repositioning.
 *
 * Placement is a preference, not a guarantee. The composable
 * flips vertically when the preferred side doesn't fit and clamps
 * horizontally to the viewport with a configurable margin. The
 * resolved placement is exposed so consumers can render an arrow
 * or transition origin that points the right way.
 */
import { computed, onScopeDispose, ref, watch, type Ref } from 'vue'

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
   * source — usually `() => triggerRef.value`. */
  element: () => HTMLElement | null
}

export interface PopoverAnchorPoint {
  type: 'point'
  x: number
  y: number
}

export type PopoverAnchor = PopoverAnchorElement | PopoverAnchorPoint

export interface UsePopoverOptions {
  /** Reactive accessor for the anchor; re-read on every update. */
  anchor: () => PopoverAnchor
  /** Preferred placement. The composable may flip vertically and
   * clamp horizontally to keep the popover on-screen. Defaults
   * to `'bottom-start'`. */
  placement?: PopoverPlacement
  /** Minimum gap from the viewport edge in pixels. */
  viewportMargin?: number
  /** Set the popover's width to match the anchor element's width.
   * Only honoured for element anchors (point anchors have no
   * intrinsic width). Useful for select-style dropdowns where the
   * menu visually "extends" the trigger. */
  matchAnchorWidth?: boolean
  /** When `matchAnchorWidth` is true, never go below this width.
   * Lets icon-only triggers still produce a readable menu. */
  minWidth?: number
  /** Pixel offset between anchor and popover. */
  offset?: number
}

export interface UsePopoverReturn {
  /** Bind to the floating element's `:ref` attribute. */
  popoverRef: Ref<HTMLElement | null>
  x: Ref<number>
  y: Ref<number>
  /** When `matchAnchorWidth` is on, the resolved width to apply
   * to the popover element. Consumer applies it via inline style. */
  width: Ref<number | null>
  /** Resolved placement after flip. Same shape as the input but
   * with `top`/`bottom` swapped if the preferred side didn't fit. */
  placement: Ref<PopoverPlacement>
  /** Recompute position. Call once after mount and any time the
   * popover or anchor size changes. */
  update: () => void
}

export function usePopover(opts: UsePopoverOptions): UsePopoverReturn {
  const popoverRef = ref<HTMLElement | null>(null)
  const x = ref(0)
  const y = ref(0)
  const width = ref<number | null>(null)
  const preferredPlacement = computed<PopoverPlacement>(
    () => opts.placement ?? 'bottom-start',
  )
  const placement = ref<PopoverPlacement>(preferredPlacement.value)

  const margin = opts.viewportMargin ?? 8
  const offset = opts.offset ?? 0

  function update() {
    const el = popoverRef.value
    if (!el) return
    const anchor = opts.anchor()
    const popRect = el.getBoundingClientRect()
    const vw = window.innerWidth
    const vh = window.innerHeight

    // Resolve anchor to a viewport rect.
    let anchorLeft: number
    let anchorTop: number
    let anchorRight: number
    let anchorBottom: number
    let anchorWidth: number
    if (anchor.type === 'element') {
      const node = anchor.element()
      if (!node) return
      const r = node.getBoundingClientRect()
      anchorLeft = r.left
      anchorTop = r.top
      anchorRight = r.right
      anchorBottom = r.bottom
      anchorWidth = r.width
      if (opts.matchAnchorWidth) {
        width.value = Math.max(opts.minWidth ?? 0, r.width)
      } else {
        width.value = null
      }
    } else {
      anchorLeft = anchorRight = anchor.x
      anchorTop = anchorBottom = anchor.y
      anchorWidth = 0
      width.value = null
    }

    // Recompute popover width if matchAnchorWidth set the value
    // (so the horizontal alignment math uses the resolved width).
    const popWidth = width.value ?? popRect.width
    const popHeight = popRect.height

    // ---------- Vertical: flip if the preferred side doesn't fit
    const prefersBottom =
      preferredPlacement.value.startsWith('bottom') ||
      preferredPlacement.value === 'bottom'
    const spaceBelow = vh - anchorBottom - margin
    const spaceAbove = anchorTop - margin
    let resolvedPlacement = preferredPlacement.value
    let top: number
    if (prefersBottom) {
      if (popHeight > spaceBelow && popHeight <= spaceAbove) {
        top = anchorTop - popHeight - offset
        resolvedPlacement = preferredPlacement.value.replace(
          'bottom',
          'top',
        ) as PopoverPlacement
      } else {
        top = anchorBottom + offset
      }
    } else {
      if (popHeight > spaceAbove && popHeight <= spaceBelow) {
        top = anchorBottom + offset
        resolvedPlacement = preferredPlacement.value.replace(
          'top',
          'bottom',
        ) as PopoverPlacement
      } else {
        top = anchorTop - popHeight - offset
      }
    }

    // ---------- Horizontal: align based on -start / -end / center
    let left: number
    if (preferredPlacement.value.endsWith('-end')) {
      left = anchorRight - popWidth
    } else if (
      preferredPlacement.value === 'top' ||
      preferredPlacement.value === 'bottom'
    ) {
      left = anchorLeft + (anchorWidth - popWidth) / 2
    } else {
      left = anchorLeft
    }

    // Clamp to viewport on both axes.
    if (left + popWidth > vw - margin) left = vw - popWidth - margin
    if (left < margin) left = margin
    if (top + popHeight > vh - margin) top = vh - popHeight - margin
    if (top < margin) top = margin

    x.value = left
    y.value = top
    placement.value = resolvedPlacement
  }

  // Re-run when the popover element mounts or the anchor function
  // result changes. The consumer drives the *trigger* for opening
  // (props.open) — this composable just makes sure that whenever
  // there's something to position, it's positioned correctly.
  watch(popoverRef, () => update())

  // Best-effort resize handling: if the popover content reflows
  // (image loads, slot grows), reposition.
  let resizeObserver: ResizeObserver | null = null
  watch(popoverRef, (el, _old, onCleanup) => {
    if (resizeObserver) {
      resizeObserver.disconnect()
      resizeObserver = null
    }
    if (el && typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(() => update())
      resizeObserver.observe(el)
      onCleanup(() => {
        resizeObserver?.disconnect()
        resizeObserver = null
      })
    }
  })

  onScopeDispose(() => {
    resizeObserver?.disconnect()
    resizeObserver = null
  })

  return { popoverRef, x, y, width, placement, update }
}
