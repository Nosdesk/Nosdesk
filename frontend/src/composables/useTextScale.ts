/**
 * The text scale in force, as the type tokens read it.
 *
 * `--text-scale` on the root (main.css) is the one number every type token
 * multiplies by: the person's own size or the device's, held to 1 to 2. This
 * reads it back from the root's computed style, so anything that has to size
 * itself in script (the reply field's line count, an email's frame) agrees
 * with the CSS by construction. It follows changes to the root's inline style
 * and attributes, which is where both inputs are written.
 */
import { readonly, ref, type Ref } from 'vue'
import { clampScale } from '@/utils/textScale'

const scale = ref(1)
let observer: MutationObserver | null = null

/** The root's `--text-scale`. Where the property is not registered (Safari
 *  before 16.4) the computed value is the unresolved `clamp()`, so the two
 *  inputs are read and clamped here instead. */
export function readTextScale(root: HTMLElement = document.documentElement): number {
  const computed = parseFloat(getComputedStyle(root).getPropertyValue('--text-scale'))
  if (Number.isFinite(computed) && computed > 0) return computed
  const user = parseFloat(root.style.getPropertyValue('--user-text-scale'))
  const device = parseFloat(root.style.getPropertyValue('--device-text-scale'))
  return clampScale(Number.isFinite(user) ? user : Number.isFinite(device) ? device : 1)
}

export function useTextScale(): Readonly<Ref<number>> {
  if (!observer && typeof document !== 'undefined' && typeof MutationObserver !== 'undefined') {
    scale.value = readTextScale()
    observer = new MutationObserver(() => {
      const next = readTextScale()
      if (next !== scale.value) scale.value = next
    })
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['style', 'class'] })
  }
  return readonly(scale)
}
