import { computed, type ComputedRef, type Ref } from 'vue'

/** The axis label size at text scale 1 on a desktop: `text-2xs`. */
const DESIGNED = 11

/**
 * The chart's axis label size in pixels, read from the container, which
 * carries `text-2xs`. The labels follow the text scale and the phone
 * floor like any other text, and the chart grows its label gutters to
 * match (`gutter(32)` is 32px at the designed size). Re-read whenever
 * the container resizes.
 */
export function useAxisFont(
  container: Ref<HTMLElement | null>,
  size: { width: Ref<number>; height: Ref<number> },
): { axisFont: ComputedRef<number>; gutter: (designed: number) => number } {
  const axisFont = computed(() => {
    // Depend on the size so a re-layout re-reads the font.
    void size.width.value
    void size.height.value
    const el = container.value
    const px = el ? parseFloat(getComputedStyle(el).fontSize) : NaN
    return Number.isFinite(px) && px > 0 ? px : DESIGNED
  })
  return { axisFont, gutter: (designed: number) => Math.round((designed * axisFont.value) / DESIGNED) }
}
