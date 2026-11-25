import { onBeforeUnmount, onMounted, ref, shallowRef, type Ref } from 'vue'

/**
 * The page title's place on a phone.
 *
 * A detail view puts `<PageTitleSlot />` in its content where the title
 * belongs (first, or under a document's breadcrumb). SiteHeader teleports
 * its editable title into it on a phone, so the title is still one
 * component with one set of handlers, and shows a one-line copy in the bar
 * once the slot has scrolled out of view.
 *
 * The same shape as `useHeaderActionSlot`, the other way round: one module
 * ref, written by whichever view is on screen, cleared when it unmounts.
 */
const target = shallowRef<HTMLElement | null>(null)
const away = ref(false)
// False until the first reading, so a page restored mid-scroll shows the
// bar's copy at once instead of fading it in.
const settled = ref(false)

/** For the header: where to put the title, and whether it is out of view. */
export const pageTitleTarget = target as Readonly<Ref<HTMLElement | null>>
export const pageTitleAway = away as Readonly<Ref<boolean>>
export const pageTitleSettled = settled as Readonly<Ref<boolean>>

/** Bring the title back, for a tap on the bar's copy. */
export function scrollToPageTitle(): void {
  const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false
  target.value?.scrollIntoView({ block: 'end', behavior: reduced ? 'auto' : 'smooth' })
}

/** Register `el` as the slot for as long as the caller is mounted. */
export function usePageTitleSlot(el: Ref<HTMLElement | null>): void {
  let observer: IntersectionObserver | null = null
  let frame = 0
  // The element this caller registered, so an unmount that lands after the
  // next view's mount leaves that view's slot alone.
  let mine: HTMLElement | null = null

  onMounted(() => {
    if (!el.value) return
    mine = el.value
    target.value = mine
    away.value = false
    settled.value = false
    if (typeof IntersectionObserver === 'undefined') return
    // No root: the reading is against the screen, clipped by whatever
    // scrolls the view, so it holds for a view that scrolls itself and for
    // one with a scroller inside it (a document).
    observer = new IntersectionObserver(([entry]) => {
      if (!entry) return
      const box = entry.boundingClientRect
      // Out of view above, with nothing of the last line left. A slot with
      // no box (wider than a phone, where it is hidden) is never away.
      away.value = !entry.isIntersecting && box.height > 0 && box.top < window.innerHeight / 2
      if (!settled.value && !frame) {
        frame = requestAnimationFrame(() => {
          frame = 0
          settled.value = true
        })
      }
    })
    observer.observe(mine)
  })

  onBeforeUnmount(() => {
    observer?.disconnect()
    observer = null
    if (frame) cancelAnimationFrame(frame)
    frame = 0
    if (mine && target.value === mine) {
      // The title leaves with the claim, while the page may still be fading
      // out (AppShell's out-in transition). Hold the slot's height so the
      // rest of the leaving page stays where it was.
      mine.style.minHeight = `${mine.offsetHeight}px`
      target.value = null
      away.value = false
      settled.value = false
    }
    mine = null
  })
}
