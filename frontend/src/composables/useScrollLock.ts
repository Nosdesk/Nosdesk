/**
 * Composable for preventing background scroll on iOS Safari.
 *
 * Uses the `position: fixed` technique to lock the body in place,
 * then restores the scroll position when unlocked. This is needed
 * because `overflow: hidden` alone does not prevent momentum scrolling
 * on iOS Safari.
 *
 * Usage:
 *   const { lock, unlock } = useScrollLock()
 *   // When opening a modal/dropdown:
 *   lock()
 *   // When closing:
 *   unlock()
 *
 * Or with a reactive flag:
 *   useScrollLock(isOpen)
 */
import { watch, onBeforeUnmount, type Ref } from 'vue'

let scrollPosition = 0
let lockCount = 0

function applyLock() {
  scrollPosition = window.pageYOffset
  document.body.style.overflow = 'hidden'
  document.body.style.position = 'fixed'
  document.body.style.top = `-${scrollPosition}px`
  document.body.style.left = '0'
  document.body.style.right = '0'
}

function releaseLock() {
  document.body.style.overflow = ''
  document.body.style.position = ''
  document.body.style.top = ''
  document.body.style.left = ''
  document.body.style.right = ''
  window.scrollTo(0, scrollPosition)
}

export function useScrollLock(isLocked?: Ref<boolean>) {
  let componentLocked = false

  const lock = () => {
    if (componentLocked) return
    componentLocked = true
    lockCount++
    if (lockCount === 1) {
      applyLock()
    }
  }

  const unlock = () => {
    if (!componentLocked) return
    componentLocked = false
    lockCount--
    if (lockCount === 0) {
      releaseLock()
    }
  }

  // Auto-lock/unlock when a reactive ref is provided
  if (isLocked) {
    watch(isLocked, (locked) => {
      if (locked) lock()
      else unlock()
    }, { immediate: true })
  }

  // Always clean up on unmount
  onBeforeUnmount(unlock)

  return { lock, unlock }
}
