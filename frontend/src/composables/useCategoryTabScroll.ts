import { ref, watch, onUnmounted, nextTick, type Ref } from 'vue'
import { useHorizontalScroll } from '@/composables/useHorizontalScroll'

/** Drag-scroll + scroll dots for the document icon picker category tabs. */
export function useCategoryTabScroll(
  categoryTabsRef: Ref<HTMLElement | null>,
  activeCategory: Ref<string>,
  categoryKeys: Ref<readonly string[]>,
) {
  const { canScrollLeft, canScrollRight, isOverflowing, updateScrollState } = useHorizontalScroll(categoryTabsRef)

  const activeCategoryDotIndex = ref(0)
  const isDragging = ref(false)
  const startX = ref(0)
  const scrollLeft = ref(0)
  const hasDragged = ref(false)

  function categoryTabButtons(): HTMLElement[] {
    if (!categoryTabsRef.value) return []
    return Array.from(categoryTabsRef.value.querySelectorAll('button'))
  }

  function syncCategoryDotFromScroll() {
    const container = categoryTabsRef.value
    const buttons = categoryTabButtons()
    if (!container || buttons.length === 0) return

    const scrollLeftPos = container.scrollLeft
    let index = 0
    for (let i = 0; i < buttons.length; i++) {
      const tab = buttons[i]
      if (tab.offsetLeft + tab.offsetWidth > scrollLeftPos + 4) {
        index = i
        break
      }
      index = i
    }
    activeCategoryDotIndex.value = index
  }

  function onCategoryTabsScroll() {
    updateScrollState()
    syncCategoryDotFromScroll()
  }

  watch(activeCategory, (key) => {
    const index = categoryKeys.value.indexOf(key)
    if (index >= 0) activeCategoryDotIndex.value = index
  })

  function handleMouseDown(e: MouseEvent) {
    if (!categoryTabsRef.value) return
    isDragging.value = true
    hasDragged.value = false
    startX.value = e.clientX
    scrollLeft.value = categoryTabsRef.value.scrollLeft
    categoryTabsRef.value.style.cursor = 'grabbing'
    document.addEventListener('mouseup', handleGlobalMouseUp)
    document.addEventListener('mousemove', handleGlobalMouseMove)
  }

  function handleGlobalMouseUp() {
    if (isDragging.value) {
      isDragging.value = false
      if (categoryTabsRef.value) {
        categoryTabsRef.value.style.cursor = 'grab'
      }
    }
    document.removeEventListener('mouseup', handleGlobalMouseUp)
    document.removeEventListener('mousemove', handleGlobalMouseMove)
    setTimeout(() => {
      hasDragged.value = false
    }, 0)
  }

  function handleGlobalMouseMove(e: MouseEvent) {
    if (!isDragging.value || !categoryTabsRef.value) return
    e.preventDefault()
    const walk = startX.value - e.clientX
    if (Math.abs(walk) > 3) {
      hasDragged.value = true
    }
    categoryTabsRef.value.scrollLeft = scrollLeft.value + walk
    syncCategoryDotFromScroll()
  }

  function handleWheel(e: WheelEvent) {
    if (!categoryTabsRef.value || !isOverflowing.value) return
    e.preventDefault()
    const delta = e.deltaY !== 0 ? e.deltaY : e.deltaX
    categoryTabsRef.value.scrollLeft += delta
  }

  function scrollToCategoryIndex(index: number) {
    const button = categoryTabButtons()[index]
    if (!button) return
    button.scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'start' })
    activeCategoryDotIndex.value = index
  }

  function refreshScrollState() {
    nextTick(() => {
      updateScrollState()
      syncCategoryDotFromScroll()
    })
  }

  onUnmounted(() => {
    document.removeEventListener('mouseup', handleGlobalMouseUp)
    document.removeEventListener('mousemove', handleGlobalMouseMove)
  })

  return {
    canScrollLeft,
    canScrollRight,
    isOverflowing,
    activeCategoryDotIndex,
    hasDragged,
    onCategoryTabsScroll,
    handleMouseDown,
    handleWheel,
    scrollToCategoryIndex,
    refreshScrollState,
  }
}
