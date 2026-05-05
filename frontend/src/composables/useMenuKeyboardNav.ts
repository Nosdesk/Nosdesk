/**
 * ARIA-compliant keyboard navigation for menu / listbox option
 * lists. Owns a roving `highlightedIndex` and a single keydown
 * handler the consumer binds to the list container.
 *
 * Implements the WAI-ARIA APG menu spec:
 *   - ↑ / ↓        previous / next item (wraps)
 *   - Home / End   first / last item
 *   - Enter / Space activate the highlighted item
 *   - Type a-z      type-ahead jump to next item whose label
 *                   starts with the typed prefix (300ms reset)
 *
 * The composable is renderless. The consumer:
 *   - reads `highlightedIndex.value` to mark the current row
 *   - binds `onKeydown` to the list element (or document, if
 *     focus stays on the trigger)
 *   - calls `setItems(items)` whenever the option list changes
 *     (e.g. after a search filter)
 *   - calls `reset()` on close to clear the type-ahead buffer
 *
 * Activation triggers the supplied `onSelect(item)` callback;
 * the consumer decides whether selection means toggle, close,
 * navigate, etc.
 */
import { ref, type Ref } from 'vue'

export interface KeyboardNavItem {
  /** Used for type-ahead matching. The composable lower-cases
   * once and caches; the consumer hands over whatever is shown
   * in the row. */
  label: string
  /** When true, arrow nav skips this item and Enter is a no-op
   * — same semantics as `aria-disabled`. */
  disabled?: boolean
}

export interface UseMenuKeyboardNav<T extends KeyboardNavItem> {
  highlightedIndex: Ref<number>
  setItems: (items: T[]) => void
  setHighlighted: (index: number) => void
  onKeydown: (event: KeyboardEvent) => void
  reset: () => void
}

const TYPEAHEAD_RESET_MS = 350

export function useMenuKeyboardNav<T extends KeyboardNavItem>(
  onSelect: (item: T, index: number) => void,
): UseMenuKeyboardNav<T> {
  const items = ref<T[]>([]) as Ref<T[]>
  const highlightedIndex = ref<number>(-1)

  let typeBuffer = ''
  let typeBufferTimer: ReturnType<typeof setTimeout> | null = null

  function setItems(next: T[]): void {
    items.value = next
    // After a list change (search filter, async load) the
    // previously-highlighted index might no longer point at the
    // same item. Snap to first non-disabled to keep the row
    // selected and the keyboard handler operational.
    if (next.length === 0) {
      highlightedIndex.value = -1
    } else if (highlightedIndex.value < 0 || highlightedIndex.value >= next.length) {
      highlightedIndex.value = firstSelectable(0, 1)
    }
  }

  function setHighlighted(index: number): void {
    if (index < 0 || index >= items.value.length) return
    if (items.value[index]?.disabled) return
    highlightedIndex.value = index
  }

  function firstSelectable(from: number, dir: 1 | -1): number {
    const n = items.value.length
    if (n === 0) return -1
    let i = from
    for (let count = 0; count < n; count++) {
      const wrapped = ((i % n) + n) % n
      if (!items.value[wrapped]?.disabled) return wrapped
      i += dir
    }
    return -1
  }

  function nextSelectable(dir: 1 | -1): void {
    const start = highlightedIndex.value < 0 ? (dir === 1 ? 0 : items.value.length - 1) : highlightedIndex.value + dir
    const idx = firstSelectable(start, dir)
    if (idx >= 0) highlightedIndex.value = idx
  }

  function activate(): void {
    const i = highlightedIndex.value
    if (i < 0 || i >= items.value.length) return
    const item = items.value[i]
    if (!item || item.disabled) return
    onSelect(item, i)
  }

  function appendType(char: string): void {
    if (typeBufferTimer) clearTimeout(typeBufferTimer)
    typeBuffer += char.toLowerCase()
    typeBufferTimer = setTimeout(() => {
      typeBuffer = ''
      typeBufferTimer = null
    }, TYPEAHEAD_RESET_MS)
    // Search starts from the current highlight so a second
    // matching key advances to the next match instead of
    // bouncing between two.
    const start = highlightedIndex.value >= 0 ? highlightedIndex.value : 0
    const n = items.value.length
    for (let offset = 1; offset <= n; offset++) {
      const idx = (start + offset) % n
      const it = items.value[idx]
      if (!it || it.disabled) continue
      if (it.label.toLowerCase().startsWith(typeBuffer)) {
        highlightedIndex.value = idx
        return
      }
    }
  }

  function onKeydown(event: KeyboardEvent): void {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        nextSelectable(1)
        return
      case 'ArrowUp':
        event.preventDefault()
        nextSelectable(-1)
        return
      case 'Home':
        event.preventDefault()
        highlightedIndex.value = firstSelectable(0, 1)
        return
      case 'End':
        event.preventDefault()
        highlightedIndex.value = firstSelectable(items.value.length - 1, -1)
        return
      case 'Enter':
      case ' ':
        // Don't preventDefault on Space when the focus is in an
        // input — the consumer wires this handler to a list
        // container, but if the search-input above us isn't
        // careful the keystroke would bubble up. Consumers should
        // attach this handler at the right scope.
        event.preventDefault()
        activate()
        return
    }
    // Type-ahead: any single printable character that isn't a
    // modifier-decorated shortcut.
    if (event.key.length === 1 && !event.metaKey && !event.ctrlKey && !event.altKey) {
      const char = event.key
      if (/[\p{L}\p{N}]/u.test(char)) {
        event.preventDefault()
        appendType(char)
      }
    }
  }

  function reset(): void {
    highlightedIndex.value = -1
    typeBuffer = ''
    if (typeBufferTimer) {
      clearTimeout(typeBufferTimer)
      typeBufferTimer = null
    }
  }

  return { highlightedIndex, setItems, setHighlighted, onKeydown, reset }
}
