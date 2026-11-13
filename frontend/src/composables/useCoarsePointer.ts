import { getCurrentScope, onScopeDispose, readonly, ref, type Ref } from 'vue'

/**
 * Whether the device's primary input is touch: a coarse pointer that can't
 * hover, which is a phone or a tablet without a mouse. Chrome that depends
 * on hover or a precise pointer swaps for touch-sized or native controls
 * there. Resolved during setup so the first render is right, and reactive,
 * so a tablet that gains a mouse switches over.
 */
export const COARSE_POINTER_QUERY = '(hover: none) and (pointer: coarse)'

export function useCoarsePointer(): Readonly<Ref<boolean>> {
  const mql =
    typeof window === 'undefined' || typeof window.matchMedia !== 'function'
      ? null
      : window.matchMedia(COARSE_POINTER_QUERY)
  const coarse = ref(mql?.matches ?? false)
  // Only a scope can take the listener down again, so outside one the first
  // answer stands.
  if (mql && getCurrentScope()) {
    const sync = (e: MediaQueryListEvent) => {
      coarse.value = e.matches
    }
    mql.addEventListener('change', sync)
    onScopeDispose(() => mql.removeEventListener('change', sync))
  }
  return readonly(coarse)
}
