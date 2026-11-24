<script setup lang="ts">
/**
 * Sort and Grouping sections for the tickets list. One markup for
 * both places they appear: the Display menu on desktop, and the top
 * of the filter sheet on a phone (where the Display menu is hidden).
 * Same chip-grid and segmented styles the Display menu already uses
 * for grouping and density. Sort sits on Grouping's three columns, so
 * the two grids share their column lines. Manual (the hand-placed
 * order) has no direction, so the direction control is off while it is
 * the sort; it keeps its place, and nothing in the menu moves.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { SORT_OPTIONS, type SortDir } from '@/composables/useTicketsSort'
import { MANUAL_SORT } from '@nosdesk/core/sync/views/planOrder'
import type { GroupBy } from '@/composables/useTicketsGrouping'

const props = defineProps<{
  sortField: string
  sortDir: SortDir
  groupBy: GroupBy
}>()

const emit = defineEmits<{
  (e: 'set-sort', field: string, dir: SortDir): void
  (e: 'set-group-by', value: GroupBy): void
}>()

const fluent = useFluent()

const sortOptions = computed(() =>
  SORT_OPTIONS.map((o) => ({ ...o, label: fluent.$t(o.labelKey) })),
)

const dirOptions = computed<ReadonlyArray<{ value: SortDir; label: string }>>(() => [
  { value: 'asc', label: fluent.$t('views-display-menu-sort-asc') },
  { value: 'desc', label: fluent.$t('views-display-menu-sort-desc') },
])

const manual = computed(() => props.sortField === MANUAL_SORT)

const groupOptions = computed<ReadonlyArray<{ value: GroupBy; label: string }>>(() => [
  { value: 'none', label: fluent.$t('views-display-menu-group-none') },
  { value: 'status', label: fluent.$t('views-display-menu-group-status') },
  { value: 'due', label: fluent.$t('views-display-menu-group-due') },
  { value: 'priority', label: fluent.$t('views-display-menu-group-priority') },
  { value: 'assignee', label: fluent.$t('views-display-menu-group-assignee') },
  { value: 'sla', label: fluent.$t('views-display-menu-group-sla') },
  { value: 'cycle', label: fluent.$t('views-display-menu-group-cycle') },
])

/** Picking a field starts it in its natural direction (soonest due,
 * most urgent, newest); picking the active field again keeps it. */
function pickField(field: string, dir: SortDir): void {
  if (field === props.sortField) return
  emit('set-sort', field, dir)
}
</script>

<template>
  <!-- Sort -->
  <section class="px-3 pb-2" data-sort-section>
    <h3 class="text-3xs uppercase tracking-wide font-semibold text-tertiary mb-1.5">
      {{ $t('views-display-menu-sort') }}
    </h3>
    <div class="flex flex-col gap-1.5">
      <div class="grid grid-cols-3 gap-1">
        <button
          v-for="opt in sortOptions"
          :key="opt.field"
          type="button"
          class="text-2xs px-2 py-1 rounded transition-colors text-center"
          :class="sortField === opt.field
            ? 'bg-accent/10 text-accent font-medium'
            : 'text-secondary hover:bg-surface-hover'"
          :aria-pressed="sortField === opt.field"
          @click.stop="pickField(opt.field, opt.dir)"
        >{{ opt.label }}</button>
      </div>
      <div
        class="inline-flex w-full items-center rounded-md border border-subtle overflow-hidden transition-opacity"
        :class="manual && 'opacity-40'"
        role="group"
        :aria-label="$t('views-display-menu-sort-direction-aria')"
        :aria-disabled="manual || undefined"
      >
        <button
          v-for="opt in dirOptions"
          :key="opt.value"
          type="button"
          class="flex-1 text-2xs px-2 py-1 transition-colors"
          :class="!manual && sortDir === opt.value
            ? 'bg-accent/10 text-accent font-medium'
            : manual ? 'text-secondary' : 'text-secondary hover:bg-surface-hover'"
          :aria-pressed="!manual && sortDir === opt.value"
          :disabled="manual"
          @click.stop="emit('set-sort', sortField, opt.value)"
        >{{ opt.label }}</button>
      </div>
    </div>
  </section>

  <!-- Grouping -->
  <section class="px-3 pb-2 border-t border-subtle pt-2">
    <h3 class="text-3xs uppercase tracking-wide font-semibold text-tertiary mb-1.5">
      {{ $t('views-display-menu-grouping') }}
    </h3>
    <div class="grid grid-cols-3 gap-1">
      <button
        v-for="opt in groupOptions"
        :key="opt.value"
        type="button"
        class="text-2xs px-2 py-1 rounded transition-colors text-center"
        :class="groupBy === opt.value
          ? 'bg-accent/10 text-accent font-medium'
          : 'text-secondary hover:bg-surface-hover'"
        :aria-pressed="groupBy === opt.value"
        @click.stop="emit('set-group-by', opt.value)"
      >{{ opt.label }}</button>
    </div>
  </section>
</template>
