/**
 * Focus trap, Escape-to-close, and focus restore for modal dialogs.
 * Keeps Modal.vue focused on layout chrome.
 */
import { ref, watch, nextTick, onMounted, onUnmounted, type Ref } from 'vue'

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'textarea:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',')

export interface UseModalDialog {
  dialogRef: Ref<HTMLElement | null>
  onTrapKeydown: (e: KeyboardEvent) => void
}

export function useModalDialog(
  show: Ref<boolean>,
  onClose: () => void,
): UseModalDialog {
  const dialogRef = ref<HTMLElement | null>(null)
  let previouslyFocused: HTMLElement | null = null

  function focusableInDialog(): HTMLElement[] {
    const root = dialogRef.value
    if (!root) return []
    return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
      (el) => !el.hasAttribute('disabled') && el.offsetParent !== null,
    )
  }

  function onTrapKeydown(e: KeyboardEvent): void {
    if (e.key !== 'Tab') return
    const elements = focusableInDialog()
    if (elements.length === 0) {
      e.preventDefault()
      dialogRef.value?.focus()
      return
    }
    const first = elements[0]
    const last = elements[elements.length - 1]
    const active = document.activeElement as HTMLElement | null
    if (e.shiftKey) {
      if (active === first || !dialogRef.value?.contains(active)) {
        e.preventDefault()
        last.focus()
      }
    } else if (active === last) {
      e.preventDefault()
      first.focus()
    }
  }

  async function moveFocusIntoDialog(): Promise<void> {
    previouslyFocused = (document.activeElement as HTMLElement | null) ?? null
    await nextTick()
    const preferred = dialogRef.value?.querySelector<HTMLElement>('[autofocus]')
    if (preferred) {
      preferred.focus()
      return
    }
    const elements = focusableInDialog()
    ;(elements[0] ?? dialogRef.value)?.focus()
  }

  function restoreFocus(): void {
    if (!previouslyFocused) return
    const target = previouslyFocused
    previouslyFocused = null
    nextTick(() => target.focus())
  }

  function onEscape(e: KeyboardEvent): void {
    if (e.key !== 'Escape' || !show.value) return
    if (document.querySelector('[popover]:popover-open, .popover-inner--visible')) return
    onClose()
  }

  onMounted(() => {
    document.addEventListener('keydown', onEscape)
    if (show.value) void moveFocusIntoDialog()
  })

  onUnmounted(() => {
    document.removeEventListener('keydown', onEscape)
    restoreFocus()
  })

  watch(show, (open) => {
    if (open) void moveFocusIntoDialog()
    else restoreFocus()
  })

  return { dialogRef, onTrapKeydown }
}
