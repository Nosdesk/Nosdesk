<script setup lang="ts">
/**
 * Multi-select option list used inside filter popovers. Two
 * upgrades over a vanilla checkbox list:
 *
 * - **Search-in-list** for long sets (more than `searchThreshold`
 *   options). The input lands at the top of the popover and
 *   filters case-insensitively. Resets focus to the first match
 *   on every keystroke so Enter activates what the user is
 *   looking at, not what they highlighted three keystrokes ago.
 *
 * - **ARIA-compliant keyboard nav** via useMenuKeyboardNav:
 *     ↑/↓     move highlight (wraps)
 *     Home/End first / last
 *     Enter   toggle the highlighted option
 *     Type    jump-to-letter (when the search input isn't
 *             focused; the search input takes precedence so
 *             typing into it filters rather than jumping)
 *
 * Roving highlight is rendered as a soft accent background with
 * a light ring — visible against both light and dark themes
 * without competing with the checkbox indicator.
 */
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import Icon from '@/components/common/Icon.vue'
import { useMenuKeyboardNav, type KeyboardNavItem } from '@/composables/useMenuKeyboardNav'
import type { FilterOption } from '@/composables/useListFilters'

const props = withDefaults(defineProps<{
  options: FilterOption[]
  selected: Set<string>
  emptyMessage?: string
  /** When the option list is at least this long, render an
   * inline search input. Eight matches the Linear / Notion
   * threshold — short enough lists are noisier with a search
   * box than without. */
  searchThreshold?: number
  /** When true, focus the search input (or the first option if
   * no search box is rendered) on mount. Most filter popovers
   * want this; set false when the popover is hosted alongside
   * other inputs that should claim focus first. */
  autoFocus?: boolean
}>(), {
  searchThreshold: 8,
  autoFocus: true,
})

const emit = defineEmits<{
  (e: 'toggle', value: string): void
  (e: 'clear'): void
}>()

const query = ref<string>('')
const searchInputRef = ref<HTMLInputElement | null>(null)
const listRef = ref<HTMLDivElement | null>(null)

const showSearch = computed<boolean>(
  () => props.options.length >= props.searchThreshold,
)

const filteredOptions = computed<FilterOption[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return props.options
  return props.options.filter((o) => o.label.toLowerCase().includes(q))
})

interface NavItem extends KeyboardNavItem {
  option: FilterOption
}

const { highlightedIndex, setItems, setHighlighted, onKeydown, reset } =
  useMenuKeyboardNav<NavItem>((item) => emit('toggle', item.option.value))

watch(
  filteredOptions,
  (next) => {
    setItems(next.map((o) => ({ label: o.label, option: o })))
  },
  { immediate: true },
)

watch(query, () => {
  // Snap highlight back to the top whenever the filter shifts
  // — Enter should toggle "the first thing in the visible list"
  // rather than whatever the user highlighted before typing.
  if (filteredOptions.value.length > 0) setHighlighted(0)
})

onMounted(() => {
  if (!props.autoFocus) return
  void nextTick(() => {
    if (showSearch.value) {
      searchInputRef.value?.focus()
    } else {
      listRef.value?.focus()
    }
  })
})

function onListKeydown(e: KeyboardEvent): void {
  // Type-ahead is a hindrance when a search input is present
  // and focused — let the input own keystrokes.
  if (showSearch.value && document.activeElement === searchInputRef.value) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Enter' || e.key === 'Home' || e.key === 'End') {
      onKeydown(e)
    }
    return
  }
  onKeydown(e)
}

function onSearchKeydown(e: KeyboardEvent): void {
  // Forward arrow / Enter to the keyboard nav so the user can
  // type-then-arrow without leaving the search input.
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Enter' || e.key === 'Home' || e.key === 'End') {
    onKeydown(e)
  }
}

function isHighlighted(index: number): boolean {
  return highlightedIndex.value === index
}

watch(
  () => props.options,
  () => {
    reset()
  },
)
</script>

<template>
  <div
    ref="listRef"
    tabindex="-1"
    role="listbox"
    aria-multiselectable="true"
    class="outline-none"
    @keydown="onListKeydown"
  >
    <div v-if="showSearch" class="p-2 border-b border-subtle">
      <div class="relative">
        <Icon
          name="search"
          class="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-tertiary pointer-events-none"
        />
        <input
          ref="searchInputRef"
          v-model="query"
          type="text"
          :placeholder="$t('views-filter-value-search-placeholder')"
          class="bg-surface border border-subtle rounded-md text-xs pl-7 pr-2 h-7 w-full text-primary placeholder:text-tertiary focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-colors"
          @keydown="onSearchKeydown"
        />
      </div>
    </div>

    <div class="max-h-[18rem] overflow-y-auto py-1">
      <p
        v-if="filteredOptions.length === 0"
        class="px-3 py-2 text-xs text-tertiary italic"
      >{{ query ? $t('views-filter-value-no-matches') : (emptyMessage ?? $t('views-filter-value-no-options')) }}</p>
      <button
        v-for="(opt, i) in filteredOptions"
        :key="opt.value"
        type="button"
        role="option"
        :aria-selected="selected.has(opt.value)"
        :tabindex="isHighlighted(i) ? 0 : -1"
        :class="[
          'w-full px-3 py-1.5 grid items-center gap-x-2 text-left transition-colors duration-75',
          opt.swatchClass
            ? 'grid-cols-[auto_auto_1fr]'
            : 'grid-cols-[auto_1fr]',
          isHighlighted(i) ? 'bg-accent/10' : 'hover:bg-surface-hover',
        ]"
        @click.stop="emit('toggle', opt.value)"
        @mouseenter="setHighlighted(i)"
      >
        <!--
          Two-column (or three with a swatch) grid keeps the
          checkbox, optional colour dot, and label on row 1 with
          `items-center` so they sit on a shared optical line,
          regardless of font ascent / descent quirks. The hint
          (when present) lives on row 2 under the label, so the
          checkbox doesn't drift to the midpoint of label+hint
          the way a single-line items-center flex would.
        -->
        <span
          class="w-3.5 h-3.5 rounded border flex items-center justify-center shrink-0 transition-colors duration-75"
          :class="selected.has(opt.value) ? 'bg-accent border-accent' : 'border-default'"
        >
          <Icon
            v-if="selected.has(opt.value)"
            name="check"
            class="w-2.5 h-2.5 text-on-accent"
          />
        </span>
        <span
          v-if="opt.swatchClass"
          class="inline-block w-2 h-2 rounded-full shrink-0"
          :class="opt.swatchClass"
          aria-hidden="true"
        />
        <span class="text-xs text-primary truncate min-w-0">{{ opt.label }}</span>
        <span
          v-if="opt.hint"
          :class="[
            'text-3xs text-tertiary truncate min-w-0',
            opt.swatchClass ? 'col-start-3' : 'col-start-2',
          ]"
        >{{ opt.hint }}</span>
      </button>
    </div>
    <footer
      v-if="selected.size > 0"
      class="border-t border-subtle px-3 py-1.5 flex items-center justify-end"
    >
      <button
        type="button"
        class="text-2xs text-tertiary hover:text-primary"
        @click="emit('clear')"
      >{{ $t('views-filter-value-clear') }}</button>
    </footer>
  </div>
</template>
