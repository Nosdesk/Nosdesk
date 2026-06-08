import { watch, type Ref } from 'vue'
import { preloadTwemoji } from '@/composables/useTwemoji'
import {
  allDocumentIconEmojis,
  documentIconsForCategory,
  type DocumentIconCategoryId,
} from '@/data/documentIconCategories'

/** Warm the full catalogue when the browser is idle. */
export function preloadDocumentIconCatalogueIdle(): void {
  const run = () => { void preloadTwemoji(allDocumentIconEmojis) }
  if (typeof requestIdleCallback === 'function') {
    requestIdleCallback(run, { timeout: 4000 })
  } else {
    setTimeout(run, 250)
  }
}

/** Preload one category — useful on trigger hover before the popover opens. */
export function preloadDocumentIconCategory(category: DocumentIconCategoryId = 'suggested'): Promise<void> {
  return preloadTwemoji(documentIconsForCategory(category))
}

/**
 * Keep Twemoji SVGs ahead of the visible picker grid.
 * `immediate: true` covers popovers that mount with `active` already true.
 */
export function useDocumentIconPreload(active: Ref<boolean>, visibleIcons: Ref<readonly string[]>) {
  function preloadVisible() {
    if (visibleIcons.value.length === 0) return
    void preloadTwemoji(visibleIcons.value)
  }

  watch(
    active,
    (open) => {
      if (!open) return
      preloadVisible()
      preloadDocumentIconCatalogueIdle()
    },
    { immediate: true },
  )

  watch(visibleIcons, () => {
    if (!active.value) return
    preloadVisible()
  })
}
