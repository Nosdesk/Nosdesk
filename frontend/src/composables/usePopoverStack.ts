/**
 * Shared registry of currently-open popover wrappers, in open order.
 *
 * Popovers teleport to <body> as siblings, so DOM containment alone
 * can't tell that one popover was opened from inside another (e.g. a
 * DatePicker calendar opened from a dropdown). This stack records open
 * order instead: a popover opened while another is already open is
 * logically nested beneath it. Outside-click dismissal consults the
 * stack so a click inside a deeper (later-opened) popover never closes
 * a shallower (earlier-opened) one.
 */
const stack: HTMLElement[] = []

export function registerPopover(el: HTMLElement): void {
  if (!stack.includes(el)) stack.push(el)
}

export function unregisterPopover(el: HTMLElement): void {
  const i = stack.indexOf(el)
  if (i !== -1) stack.splice(i, 1)
}

/** True when `target` lies inside a popover opened after `el` — that
 *  is, one nested beneath it. */
export function isInNestedPopover(el: HTMLElement, target: Node): boolean {
  const i = stack.indexOf(el)
  if (i === -1) return false
  for (let j = i + 1; j < stack.length; j += 1) {
    if (stack[j].contains(target)) return true
  }
  return false
}
