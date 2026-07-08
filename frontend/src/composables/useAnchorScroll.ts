/**
 * Active-section tracking for the dashboard's left anchor rail.
 *
 * Each section in the canvas renders an anchor marker (a hidden /
 * visible H2 with an `id`); this composable watches them via
 * IntersectionObserver and exposes a reactive `activeId` that the
 * rail uses to highlight the current anchor.
 *
 * The threshold is 0.4 — the section becomes active once 40% of it
 * is in viewport. Lower values (0.1) flicker on scroll start; higher
 * (0.6) waits too long. 0.4 matches Linear's behaviour closely.
 *
 * Callers register every anchor element they care about via the
 * returned `register(id, el)` function; the composable owns the
 * observer's lifecycle. Anchors deregister automatically on element
 * unmount.
 */
import { onBeforeUnmount, ref, type Ref } from 'vue'

const VISIBILITY_THRESHOLD = 0.4

export interface AnchorScroll {
  /** The currently most-visible anchor id; `null` until the first
   *  intersection event lands. */
  activeId: Ref<string | null>
  /** Register an anchor element so it joins the observer. Returns a
   *  teardown closure callers can hand to `onBeforeUnmount` if they
   *  want to deregister early; the composable's own unmount
   *  cleanup tears down everything regardless. */
  register: (id: string, el: Element | null) => () => void
  /** Smooth-scroll a section into view by anchor id. Used by the
   *  rail when the user clicks an anchor. */
  scrollTo: (id: string) => void
}

export function useAnchorScroll(): AnchorScroll {
  const activeId = ref<string | null>(null)
  const elementsById = new Map<string, Element>()
  // Track each element's current intersection ratio so we can pick
  // the highest ratio across multiple-section-in-viewport cases (the
  // section with the most coverage wins the active highlight, even
  // if every section in the page passes the threshold).
  const ratioById = new Map<string, number>()

  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        const id = (entry.target as HTMLElement).id
        if (!id) continue
        ratioById.set(id, entry.intersectionRatio)
      }
      // Pick the anchor with the highest intersection ratio that
      // also clears the threshold. If nothing clears the threshold,
      // fall back to the highest-ratio anchor regardless so the rail
      // still has SOMETHING highlighted as the user scrolls into a
      // sparsely-anchored region.
      let bestId: string | null = null
      let bestRatio = -1
      for (const [id, ratio] of ratioById) {
        if (ratio > bestRatio) {
          bestId = id
          bestRatio = ratio
        }
      }
      if (bestRatio >= VISIBILITY_THRESHOLD) {
        activeId.value = bestId
      } else if (bestId !== null && activeId.value === null) {
        // First-frame fallback: nothing meets the threshold yet but
        // we still want the rail to highlight something on page load.
        activeId.value = bestId
      }
    },
    {
      threshold: [0, VISIBILITY_THRESHOLD, 0.6, 1],
      // Anchor markers are short H2s; pulling the top edge of the
      // viewport down by 64px keeps the chrome row from gaming
      // intersection ratios.
      rootMargin: '-64px 0px 0px 0px',
    },
  )

  function register(id: string, el: Element | null): () => void {
    if (!el) return () => undefined
    elementsById.set(id, el)
    observer.observe(el)
    return () => {
      if (elementsById.get(id) === el) {
        elementsById.delete(id)
        ratioById.delete(id)
        observer.unobserve(el)
        if (activeId.value === id) {
          activeId.value = null
        }
      }
    }
  }

  function scrollTo(id: string): void {
    const el = elementsById.get(id)
    if (!el) return
    el.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  onBeforeUnmount(() => {
    observer.disconnect()
    elementsById.clear()
    ratioById.clear()
  })

  return { activeId, register, scrollTo }
}
