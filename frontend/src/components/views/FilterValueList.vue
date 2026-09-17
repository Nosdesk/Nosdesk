<script setup lang="ts">
/**
 * Multi-select option list used inside filter popovers, on Reka's
 * Listbox (`aria-multiselectable` listbox of `role=option` rows,
 * arrows, Home/End, Enter to toggle, typeahead). Long sets (more
 * than `searchThreshold` options) get a search input at the top,
 * a `ListboxFilter`: it owns the keyboard (arrows move the
 * highlight, Enter toggles it, the highlight snaps to the first
 * match on every keystroke) and names the highlighted row through
 * `aria-activedescendant`.
 *
 * The parent owns the selection: every change is reported as a
 * `toggle` of the one value that flipped.
 */
import { computed, nextTick, onMounted, ref } from 'vue'
import { ListboxContent, ListboxFilter, ListboxItem, ListboxRoot } from 'reka-ui'
import Icon from '@/components/common/Icon.vue'
import type { FilterOption } from '@/composables/useListFilters'

const props = withDefaults(defineProps<{
  options: FilterOption[]
  selected: Set<string>
  /** Accessible name of the list (the facet's label). */
  label?: string
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

const showSearch = computed<boolean>(
  () => props.options.length >= props.searchThreshold,
)

const filteredOptions = computed<FilterOption[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return props.options
  return props.options.filter((o) => o.label.toLowerCase().includes(q))
})

const model = computed(() => [...props.selected])

// Reka reports the whole next selection; the parent wants the one
// value that changed.
function onUpdate(next: unknown) {
  const values = Array.isArray(next) ? (next as string[]) : []
  const changed =
    values.find((v) => !props.selected.has(v)) ?? [...props.selected].find((v) => !values.includes(v))
  if (changed !== undefined) emit('toggle', changed)
}

// Without a search box focus goes to the first selected row, else
// the first row (Reka highlights it on mount without focusing).
const listbox = ref<{ highlightSelected: () => Promise<void> } | null>(null)
onMounted(async () => {
  if (!props.autoFocus || showSearch.value) return
  await nextTick()
  await listbox.value?.highlightSelected()
})
</script>

<template>
  <ListboxRoot ref="listbox" :model-value="model" multiple highlight-on-hover @update:model-value="onUpdate">
    <div v-if="showSearch" class="p-2 border-b border-subtle">
      <div class="relative">
        <Icon
          name="search"
          class="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-tertiary pointer-events-none"
        />
        <ListboxFilter
          v-model="query"
          :auto-focus="autoFocus"
          :placeholder="$t('views-filter-value-search-placeholder')"
          :aria-label="$t('views-filter-value-search-placeholder')"
          class="bg-surface border border-subtle rounded-md text-xs pl-7 pr-2 h-7 w-full text-primary placeholder:text-tertiary focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-colors"
        />
      </div>
    </div>

    <!-- With a filter the rows are not focusable, so the scrolling
         list is a tab stop of its own (axe scrollable-region-focusable). -->
    <ListboxContent as-child>
      <div
        class="max-h-[18rem] overflow-y-auto py-1 outline-none"
        :aria-label="label"
        v-bind="showSearch ? { tabindex: 0 } : {}"
      >
        <p
          v-if="filteredOptions.length === 0"
          class="px-3 py-2 text-xs text-tertiary italic"
        >{{ query ? $t('views-filter-value-no-matches') : (emptyMessage ?? $t('views-filter-value-no-options')) }}</p>
        <ListboxItem v-for="opt in filteredOptions" :key="opt.value" as-child :value="opt.value">
          <button
            type="button"
            :class="[
              'w-full px-3 py-1.5 grid items-center gap-x-2 text-left transition-colors duration-75 outline-none',
              opt.swatchClass
                ? 'grid-cols-[auto_auto_1fr]'
                : 'grid-cols-[auto_1fr]',
              'hover:bg-surface-hover data-[highlighted]:bg-accent/10',
            ]"
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
        </ListboxItem>
      </div>
    </ListboxContent>
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
  </ListboxRoot>
</template>
