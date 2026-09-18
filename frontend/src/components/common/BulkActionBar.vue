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

The pill is a Reka Toolbar: one tab stop, arrows walk the controls.
Consumers own the buttons + handlers in the `#actions` slot and wrap
each in `ToolbarButton as-child` (passing `disabled` to it too, so the
roving focus skips it), or it stays outside the group as its own tab
stop. The bar knows nothing about what the actions DO.
-->
<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { ToolbarButton, ToolbarRoot } from 'reka-ui'

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
    /** FTL key for the "N selected" copy. Should use a Fluent plural
     *  selector on `$count` so non-English locales pluralise the
     *  noun correctly. Defaults to a generic "N selected" key. */
    selectionCopyKey?: string
    /** FTL key for the "All N selected" copy (different key because
     *  some locales reorder the count vs the noun). Defaults to
     *  a generic "All N selected" key. */
    allSelectedCopyKey?: string
  }>(),
  {
    totalCount: 0,
    isAllMatchingSelected: false,
    selectionCopyKey: 'bulk-bar-selected-generic',
    allSelectedCopyKey: 'bulk-bar-all-selected-generic',
  },
)

const emit = defineEmits<{
  'select-all-matching': []
  'clear': []
}>()

// Show "Select all N" only when there's more matching the filter
// than the user has selected and they haven't already opted in.
const showSelectAllMatching = computed(() =>
  !props.isAllMatchingSelected &&
  props.totalCount > 0 &&
  props.selectedCount < props.totalCount,
)

const countCopy = computed(() => {
  if (props.isAllMatchingSelected && props.totalCount > 0) {
    return t(props.allSelectedCopyKey, { count: props.totalCount })
  }
  return t(props.selectionCopyKey, { count: props.selectedCount })
})
</script>

<template>
  <!-- Always mounted, so the first selection is announced too; a live
       region born with the bar would say nothing until the next change. -->
  <span class="sr-only" aria-live="polite">{{ selectedCount > 0 ? countCopy : '' }}</span>
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
    >
      <ToolbarRoot
        :aria-label="t('common-bulk-actions-aria')"
        class="pointer-events-auto inline-flex items-stretch gap-2 px-2 py-1.5 rounded-full bg-surface border border-default shadow-lg"
      >
        <!-- Count pill + scope toggles -->
        <div class="flex items-center gap-3 pl-2 pr-3 border-r border-default">
          <div
            class="flex items-center justify-center min-w-6 h-6 px-2 bg-accent text-on-accent text-xs font-bold rounded-full"
          >
            {{ selectedCount }}
          </div>
          <div class="flex items-center gap-2 text-xs">
            <span class="text-secondary whitespace-nowrap">{{ countCopy }}</span>
            <ToolbarButton v-if="showSelectAllMatching" as-child>
              <button
                type="button"
                @click="emit('select-all-matching')"
                class="text-accent hover:underline whitespace-nowrap rounded focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                {{ t('bulk-bar-select-all-matching', { count: totalCount }) }}
              </button>
            </ToolbarButton>
            <ToolbarButton as-child>
              <button
                type="button"
                @click="emit('clear')"
                class="text-tertiary hover:text-secondary whitespace-nowrap rounded focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                {{ t('bulk-bar-clear') }}
              </button>
            </ToolbarButton>
          </div>
        </div>

        <!-- Consumer-owned action buttons. Recommended: 2-3 inline
             buttons + an overflow menu if you have more, per the Q5
             research findings. -->
        <div class="flex items-center gap-1 pr-1">
          <slot name="actions" :selected-count="selectedCount" :is-all-matching="isAllMatchingSelected" />
        </div>
      </ToolbarRoot>
    </div>
  </Transition>
</template>
