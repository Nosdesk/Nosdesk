<!-- Inline document icon picker grid (search, categories, emoji tiles). -->
<script setup lang="ts">
import { ref, watch, onUnmounted, nextTick } from 'vue'
import { useFluent } from 'fluent-vue'
import { useHorizontalScroll } from '@/composables/useHorizontalScroll'
import { useDocumentIconPicker } from '@/composables/useDocumentIconPicker'
import Icon from '@/components/common/Icon.vue'
import Emoji from '@/components/common/Emoji.vue'

const { $t } = useFluent()

const props = withDefaults(defineProps<{
  modelValue: string
  /** When true, preloads emoji assets for the active category. */
  active?: boolean
  /** Close the surrounding dropdown after a selection (popover mode). */
  closeOnSelect?: boolean
  gridMaxClass?: string
}>(), {
  active: true,
  closeOnSelect: true,
  gridMaxClass: 'max-h-64',
})

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
  (e: 'select', value: string): void
}>()

const searchQuery = ref('')
const activeCategory = ref('suggested')
const categoryTabsRef = ref<HTMLElement | null>(null)
const pickerActive = ref(props.active)

watch(() => props.active, (open) => {
  pickerActive.value = open
  if (open) {
    searchQuery.value = ''
    activeCategory.value = 'suggested'
    nextTick(() => {
      updateScrollState()
      syncCategoryDotFromScroll()
    })
  }
}, { immediate: true })

const { canScrollLeft, canScrollRight, isOverflowing, updateScrollState } = useHorizontalScroll(categoryTabsRef)

const {
  iconCategories,
  categoryKeys,
  filteredIcons,
  allIcons,
} = useDocumentIconPicker({
  activeCategory,
  searchQuery,
  showDropdown: pickerActive,
})

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

function selectIcon(icon: string) {
  emit('update:modelValue', icon)
  emit('select', icon)
  if (props.closeOnSelect) {
    searchQuery.value = ''
  }
}

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

onUnmounted(() => {
  document.removeEventListener('mouseup', handleGlobalMouseUp)
  document.removeEventListener('mousemove', handleGlobalMouseMove)
})
</script>

<template>
  <div class="flex flex-col min-w-0 rounded-xl border border-default bg-surface overflow-hidden">
    <div class="p-3 border-b border-default">
      <div class="relative">
        <span class="absolute left-3 top-1/2 -translate-y-1/2 text-tertiary inline-flex">
          <Icon name="search" />
        </span>
        <input
          v-model="searchQuery"
          type="text"
          :placeholder="$t('doc-icon-selector-search-placeholder')"
          class="w-full pl-10 pr-4 py-2 text-sm bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent/50 focus:border-accent"
        />
      </div>
    </div>

    <div v-if="!searchQuery" class="relative border-b border-default">
      <div
        class="absolute left-0 top-0 bottom-0 w-6 bg-gradient-to-r from-surface to-transparent pointer-events-none z-10 transition-opacity duration-200"
        :class="canScrollLeft ? 'opacity-100' : 'opacity-0'"
      />

      <div
        ref="categoryTabsRef"
        class="category-tabs flex gap-1 px-3 py-2 overflow-x-auto cursor-grab select-none"
        @mousedown="handleMouseDown"
        @wheel="handleWheel"
        @scroll="onCategoryTabsScroll"
      >
        <button
          v-for="(category, key) in iconCategories"
          :key="key"
          type="button"
          @click.stop="!hasDragged && (activeCategory = key)"
          class="px-3 py-1.5 text-xs font-medium rounded-md whitespace-nowrap transition-colors shrink-0"
          :class="activeCategory === key
            ? 'bg-accent text-on-accent'
            : 'text-secondary hover:text-primary hover:bg-surface-hover'"
        >
          {{ category.label }}
        </button>
      </div>

      <div
        class="absolute right-0 top-0 bottom-0 w-6 bg-gradient-to-l from-surface to-transparent pointer-events-none z-10 transition-opacity duration-200"
        :class="canScrollRight ? 'opacity-100' : 'opacity-0'"
      />

      <div v-if="isOverflowing" class="flex justify-center gap-1 py-1.5 bg-surface-alt">
        <button
          v-for="(key, index) in categoryKeys"
          :key="key"
          type="button"
          class="w-1.5 h-1.5 p-0 border-0 rounded-full bg-tertiary transition-all duration-200 cursor-pointer hover:scale-125 shrink-0"
          :class="index === activeCategoryDotIndex ? 'opacity-100' : 'opacity-30 hover:opacity-60'"
          @click.stop="scrollToCategoryIndex(index)"
          :aria-label="$t('doc-icon-selector-scroll-dot-aria', { index: index + 1 })"
        />
      </div>
    </div>

    <div class="p-3 overflow-y-auto" :class="gridMaxClass">
      <div v-if="searchQuery && filteredIcons.length === 0" class="py-8 text-center text-tertiary text-sm">
        {{ $t('doc-icon-selector-empty') }}
      </div>
      <div v-else class="grid grid-cols-8 gap-1">
        <button
          v-for="icon in filteredIcons"
          :key="icon"
          type="button"
          @click.stop="selectIcon(icon)"
          class="flex items-center justify-center w-8 h-8 text-xl rounded-md transition-all duration-100 hover:bg-surface-hover hover:scale-110 active:scale-95"
          :class="modelValue === icon ? 'bg-accent/20 ring-2 ring-accent' : ''"
        >
          <Emoji :emoji="icon" size="xl" eager />
        </button>
      </div>
    </div>

    <div class="px-3 py-2 border-t border-default bg-surface-alt flex items-center justify-between">
      <span class="text-xs text-tertiary">{{ $t('doc-icon-selector-footer-hint') }}</span>
      <button
        type="button"
        @click.stop="selectIcon(allIcons[Math.floor(Math.random() * allIcons.length)])"
        class="px-2 py-1 text-xs font-medium text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors"
      >
        {{ $t('doc-icon-selector-random') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.category-tabs {
  -ms-overflow-style: none;
  scrollbar-width: none;
}
.category-tabs::-webkit-scrollbar {
  display: none;
}
.category-tabs button {
  cursor: pointer;
}
</style>
