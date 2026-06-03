import { onBeforeUnmount, ref, watch, type Ref } from 'vue'

/**
 * Reactive content-box size of an element, tracked via ResizeObserver.
 *
 * Pass a template ref; the returned `width` / `height` (CSS pixels)
 * stay current as the element resizes — window resize, sidebar
 * collapse, widget drag-resize, container reflow, anything. Components
 * that need real pixel dimensions (responsive SVG charts that draw 1:1
 * to avoid `preserveAspectRatio` distortion) read these instead of
 * hand-rolling an observer.
 *
 * The observer is (re)attached via a `flush: 'post'` watch on the
 * target ref rather than a one-shot `onMounted`, so it works whether
 * the element is present at mount or appears later (behind a `v-if`,
 * inside a `<Transition>`), and re-binds if the ref swaps elements.
 */
export function useElementSize(target: Ref<HTMLElement | null>): {
  width: Ref<number>
  height: Ref<number>
} {
  const width = ref(0)
  const height = ref(0)
  let observer: ResizeObserver | null = null

  const disconnect = () => {
    observer?.disconnect()
    observer = null
  }

  watch(
    target,
    (el) => {
      disconnect()
      if (!el) return
      // Seed synchronously so the first paint after the element
      // appears already has real dimensions (no zero-size flash).
      const rect = el.getBoundingClientRect()
      width.value = rect.width
      height.value = rect.height
      observer = new ResizeObserver((entries) => {
        const box = entries[0]?.contentRect
        if (box) {
          width.value = box.width
          height.value = box.height
        }
      })
      observer.observe(el)
    },
    { immediate: true, flush: 'post' },
  )

  onBeforeUnmount(disconnect)

  return { width, height }
}
