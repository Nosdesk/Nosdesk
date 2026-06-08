<!-- Inline document icon picker grid (search, categories, emoji tiles). -->
<script setup lang="ts">
import { ref, watch, toRef } from 'vue'
import { useDocumentIconPicker } from '@/composables/useDocumentIconPicker'
import { useDocumentIconPreload } from '@/composables/useDocumentIconPreload'
import { useCategoryTabScroll } from '@/composables/useCategoryTabScroll'
import Icon from '@/components/common/Icon.vue'
import Emoji from '@/components/common/Emoji.vue'

const props = withDefaults(defineProps<{
  modelValue: string
  /** When true, preloads emoji assets for the active category. */
  active?: boolean
  /** Close the surrounding dropdown after a selection (popover mode). */
  closeOnSelect?: boolean
  gridMaxClass?: string
  /** Drop outer chrome when nested inside another panel (e.g. appearance modal). */
  embedded?: boolean
  /** Fill parent height; grid scrolls internally instead of using max-height. */
  fillHeight?: boolean
}>(), {
  active: true,
  closeOnSelect: true,
  gridMaxClass: 'max-h-64',
  embedded: false,
  fillHeight: false,
})

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
  (e: 'select', value: string): void
}>()

const active = toRef(props, 'active')
const searchQuery = ref('')
const activeCategory = ref('suggested')
const categoryTabsRef = ref<HTMLElement | null>(null)

const { iconCategories, categoryKeys, filteredIcons, allIcons } = useDocumentIconPicker({
  activeCategory,
  searchQuery,
})

useDocumentIconPreload(active, filteredIcons)

const {
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
} = useCategoryTabScroll(categoryTabsRef, activeCategory, categoryKeys)

watch(active, (open) => {
  if (!open) return
  searchQuery.value = ''
  activeCategory.value = 'suggested'
  refreshScrollState()
}, { immediate: true })

function selectIcon(icon: string) {
  emit('update:modelValue', icon)
  emit('select', icon)
  if (props.closeOnSelect) {
    searchQuery.value = ''
  }
}
</script>

<template>
  <div
    class="flex flex-col min-w-0 overflow-hidden"
    :class="[
      embedded ? '' : 'rounded-xl border border-default bg-surface',
      fillHeight ? 'h-full min-h-0' : '',
    ]"
  >
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

    <div
      class="p-3 overflow-y-auto"
      :class="fillHeight ? 'flex-1 min-h-0' : gridMaxClass"
    >
      <div v-if="searchQuery && filteredIcons.length === 0" class="py-8 text-center text-tertiary text-sm">
        {{ $t('doc-icon-selector-empty') }}
      </div>
      <div v-else class="icon-picker-grid">
        <button
          v-for="icon in filteredIcons"
          :key="icon"
          type="button"
          @click.stop="selectIcon(icon)"
          class="icon-picker-cell flex items-center justify-center rounded-md transition-all duration-100 hover:bg-surface-hover hover:scale-110 active:scale-95"
          :class="modelValue === icon ? 'bg-accent/20 ring-2 ring-accent' : ''"
        >
          <Emoji :emoji="icon" size="lg" eager />
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

/* Fit as many columns as the container allows; cell size scales with width. */
.icon-picker-grid {
  container-type: inline-size;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(1.625rem, 1fr));
  gap: 0.25rem;
}

.icon-picker-cell {
  aspect-ratio: 1;
  width: 100%;
  max-width: 2rem;
  justify-self: center;
}

@container (min-width: 28rem) {
  .icon-picker-grid {
    grid-template-columns: repeat(auto-fill, minmax(1.5rem, 1fr));
  }

  .icon-picker-cell {
    max-width: 1.875rem;
  }
}
</style>
