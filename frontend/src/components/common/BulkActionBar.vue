<!--
Sticky-bottom bulk-action bar. The 2025 Linear / Asana / Notion /
Gmail pattern: a floating pill anchored to the bottom-center of the
viewport that appears when items are selected.

Why bottom-floating instead of a top inline bar (the legacy
`BulkActionsBar.vue` pattern):
 - Works at any scroll position. Top-inline bars vanish when the
   user scrolls past them, leaving the action chrome offscreen.
 - Doesn't displace the filter row or table header. The page
   layout stays stable regardless of selection state.
 - Reads visually as a "command palette for your selection",
   matching what users expect from contemporary admin tools.

Anatomy from left to right:
 - Selection count pill ("12 selected")
 - "Select all matching X" affordance (when relevant) and Clear
 - Action slot (consumer renders inline buttons + an overflow menu
   if it has more than ~3 actions, see Q5 research)

The bar is intentionally chrome-only: it knows nothing about what
the actions DO, just that there are actions to render. Consumers
own the buttons + handlers in the `#actions` slot.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const props = withDefaults(
  defineProps<{
    /** How many items are currently selected. The bar is hidden
     *  when this is 0 (slide-down animation handled by Transition). */
    selectedCount: number
    /** Total number of items matching the active filter, used to
     *  render "Select all N" and decide whether the affordance
     *  appears at all. Pass 0 to hide it entirely. */
    totalCount?: number
    /** True when the user has opted into the "all matching" scope.
     *  Bar copy switches to "All N selected" + a "Deselect all"
     *  affordance instead of "Select all matching". */
    isAllMatchingSelected?: boolean
    /** Singular item label, e.g. `"ticket"`. Pluralised for counts. */
    itemLabel?: string
  }>(),
  {
    totalCount: 0,
    isAllMatchingSelected: false,
    itemLabel: 'item',
  },
)

const emit = defineEmits<{
  'select-all-matching': []
  'clear': []
}>()

const pluralLabel = computed(() =>
  props.selectedCount === 1 ? props.itemLabel : `${props.itemLabel}s`,
)

// Show "Select all N" only when there's more matching the filter
// than the user has selected and they haven't already opted in.
const showSelectAllMatching = computed(() =>
  !props.isAllMatchingSelected &&
  props.totalCount > 0 &&
  props.selectedCount < props.totalCount,
)

const countCopy = computed(() => {
  if (props.isAllMatchingSelected && props.totalCount > 0) {
    return `All ${props.totalCount} ${pluralLabel.value} selected`
  }
  return `${props.selectedCount} ${pluralLabel.value} selected`
})
</script>

<template>
  <!--
    No Teleport: the inner div uses `position: fixed` which already
    escapes its scroll/overflow ancestors (none of which create a
    containing block via transform/filter/perspective). Teleport
    inside a KeepAlive-cached parent has documented interaction
    edge cases with route Transitions; keeping the bar inline
    avoids that whole class of bugs at zero positioning cost.
  -->
  <Transition
    enter-active-class="transition-all duration-200 ease-out"
    enter-from-class="opacity-0 translate-y-3"
    enter-to-class="opacity-100 translate-y-0"
    leave-active-class="transition-all duration-150 ease-in"
    leave-from-class="opacity-100 translate-y-0"
    leave-to-class="opacity-0 translate-y-3"
  >
    <div
      v-if="selectedCount > 0"
      class="fixed bottom-6 inset-x-0 z-overlay flex justify-center px-4 pointer-events-none"
      role="region"
      :aria-label="t('common-bulk-actions-aria')"
    >
      <div
        class="pointer-events-auto inline-flex items-stretch gap-2 px-2 py-1.5 rounded-full bg-surface border border-default shadow-lg"
      >
        <!-- Count pill + scope toggles -->
        <div class="flex items-center gap-3 pl-2 pr-3 border-r border-default">
          <div
            class="flex items-center justify-center min-w-6 h-6 px-2 bg-accent text-white text-xs font-bold rounded-full"
          >
            {{ selectedCount }}
          </div>
          <div class="flex items-center gap-2 text-xs">
            <span class="text-secondary whitespace-nowrap">{{ countCopy }}</span>
            <button
              v-if="showSelectAllMatching"
              type="button"
              @click="emit('select-all-matching')"
              class="text-accent hover:underline whitespace-nowrap"
            >
              Select all {{ totalCount }}
            </button>
            <button
              type="button"
              @click="emit('clear')"
              class="text-tertiary hover:text-secondary whitespace-nowrap"
            >
              Clear
            </button>
          </div>
        </div>

        <!-- Consumer-owned action buttons. Recommended: 2-3 inline
             buttons + an overflow menu if you have more, per the Q5
             research findings. -->
        <div class="flex items-center gap-1 pr-1">
          <slot name="actions" :selected-count="selectedCount" :is-all-matching="isAllMatchingSelected" />
        </div>
      </div>
    </div>
  </Transition>
</template>
