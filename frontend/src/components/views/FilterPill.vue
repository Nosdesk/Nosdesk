<script setup lang="ts">
/**
 * Active-filter chip rendered inline with the page title.
 *
 * Two interactive zones in one pill:
 *  - Body click  -> opens the value picker (re-edit selection)
 *  - X button    -> removes the filter entirely
 *
 * Style mirrors the reference design: bordered amber/accent pill
 * with the facet label, a colon, and a short value summary.
 *
 * Title-facet pills get a text input in the popover instead of
 * the checkbox list, so search lives in the same affordance as
 * any other filter — one mental model.
 */
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import FilterValueList from '@/components/views/FilterValueList.vue'
import type { PopoverAnchor } from '@/composables/usePopover'
import type { FilterOption, FacetKind } from '@/composables/useListFilters'

const props = defineProps<{
  /** Stable facet key (eg. "status", "warranty", "role"). Used
   *  by the consumer to map events back to the right facet. */
  facet: string
  /** Facet kind. Drives whether the popover shows a text input
   *  (kind === 'text') or the multi-select value list. */
  kind: FacetKind
  label: string
  valueSummary: string
  /** For multi facets: the option list + current selection.
   * For text facets: pass an empty array and the current text
   * via `textValue`. */
  options: FilterOption[]
  selected: Set<string>
  textValue?: string
  emptyMessage?: string
  /** Placeholder for the text-facet input. Defaults to the
   *  generic search-title placeholder string. */
  textPlaceholder?: string
}>()

const emit = defineEmits<{
  (e: 'toggle', value: string): void
  (e: 'clear'): void
  (e: 'set-text', value: string): void
  (e: 'remove'): void
}>()

const triggerRef = ref<HTMLElement | null>(null)
const open = ref(false)

const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerRef.value,
}))

const isText = computed<boolean>(() => props.kind === 'text')

function onTextInput(e: Event): void {
  const t = e.target as HTMLInputElement
  emit('set-text', t.value)
}

function onRemove(e: MouseEvent): void {
  e.stopPropagation()
  emit('remove')
}
</script>

<template>
  <div class="inline-flex">
    <div
      ref="triggerRef"
      class="inline-flex items-center h-6 rounded-md border border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300 text-[11px] overflow-hidden"
    >
      <button
        type="button"
        class="inline-flex items-center gap-1 pl-2 pr-1.5 h-full transition-colors"
        :class="open ? 'bg-amber-500/20' : 'hover:bg-amber-500/15'"
        :aria-expanded="open"
        aria-haspopup="menu"
        @click="open = !open"
      >
        <span class="font-medium">{{ label }}:</span>
        <span class="truncate max-w-[12rem]">{{ valueSummary }}</span>
      </button>
      <button
        type="button"
        class="inline-flex items-center justify-center h-full pr-1.5 pl-0.5 hover:bg-amber-500/15 transition-colors"
        :title="$t('views-filter-pill-remove-tooltip', { label })"
        @click="onRemove"
      >
        <Icon name="close" class="w-3 h-3" />
      </button>
    </div>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      :title="label"
      placement="bottom-start"
      react-to-scroll="reposition"
      :offset="4"
      role="menu"
      :auto-focus="!isText"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[16rem] max-w-[22rem]"
      @close="open = false"
    >
      <div v-if="isText" class="p-2">
        <input
          type="text"
          :value="textValue ?? ''"
          :placeholder="textPlaceholder ?? $t('views-filter-pill-search-title-placeholder')"
          class="bg-surface border border-subtle rounded-md text-xs px-2 h-7 w-full text-primary placeholder:text-tertiary focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-colors"
          @input="onTextInput"
        />
      </div>
      <FilterValueList
        v-else
        :options="options"
        :selected="selected"
        :empty-message="emptyMessage"
        @toggle="(v) => emit('toggle', v)"
        @clear="emit('clear')"
      />
    </ResponsiveMenu>
  </div>
</template>
