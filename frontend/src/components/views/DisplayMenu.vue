<script setup lang="ts">
/**
 * Tickets-table display options menu. Linear-style "Display"
 * button: one trigger that opens a popover containing every
 * presentational knob (density, column visibility, room for
 * grouping / ordering when those land). Replaces the prior
 * pattern where density + columns were two separate trigger
 * buttons in the toolbar — a 14-column table with multiple
 * trigger buttons in a row gets noisy fast.
 *
 * The popover stays open between toggles (Linear / Asana
 * convention) so multi-property adjustments don't require
 * reopening the popover for every pick.
 */
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import type { PopoverAnchor } from '@/composables/usePopover'
import {
  TICKET_COLUMNS,
  type ColumnId,
  type ListColumn,
} from '@/sync/views/ticketColumns'
import type { Density } from '@/composables/useTicketsDensity'
import type { GroupBy } from '@/composables/useTicketsGrouping'

const props = defineProps<{
  visible: ColumnId[]
  density: Density
  groupBy: GroupBy
  /** True when the active view is editable (saved view owned by
   * the user / workspace) — surfaces the "Save layout to view"
   * affordance below the column list. */
  canSaveToView?: boolean
  /** True when the local choice differs from the saved view's
   * canonical layout; drives the Save button enabled state. */
  layoutDirty?: boolean
}>()

const emit = defineEmits<{
  (e: 'toggle-column', id: ColumnId): void
  (e: 'set-density', value: Density): void
  (e: 'set-group-by', value: GroupBy): void
  (e: 'reset'): void
  (e: 'save'): void
}>()

const triggerRef = ref<HTMLElement | null>(null)
const open = ref(false)

const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerRef.value,
}))

const visibleSet = computed<Set<ColumnId>>(() => new Set(props.visible))

function isOn(col: ListColumn): boolean {
  return visibleSet.value.has(col.id)
}

const densityOptions: ReadonlyArray<{ value: Density; label: string }> = [
  { value: 'compact', label: 'Compact' },
  { value: 'cosy', label: 'Cosy' },
  { value: 'comfortable', label: 'Comfortable' },
]

const groupOptions: ReadonlyArray<{ value: GroupBy; label: string }> = [
  { value: 'none', label: 'None' },
  { value: 'status', label: 'Status' },
  { value: 'priority', label: 'Priority' },
  { value: 'assignee', label: 'Assignee' },
  { value: 'sla', label: 'SLA' },
  { value: 'cycle', label: 'Cycle' },
]
</script>

<template>
  <div class="inline-flex">
    <button
      ref="triggerRef"
      type="button"
      class="inline-flex items-center gap-1.5 text-xs px-2 py-1 rounded-md transition-colors"
      :class="open
        ? 'text-primary bg-surface-hover'
        : 'text-secondary hover:text-primary hover:bg-surface-hover'"
      :aria-expanded="open"
      aria-haspopup="menu"
      title="Display options"
      @click="open = !open"
    >
      <Icon name="settings" class="w-3.5 h-3.5" />
      <span>Display</span>
    </button>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      title="Display"
      placement="bottom-end"
      react-to-scroll="reposition"
      :offset="4"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[18rem] max-w-[22rem]"
      @close="open = false"
    >
      <div class="py-2">
        <!-- Grouping -->
        <section class="px-3 pb-2">
          <h3 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary mb-1.5">
            Grouping
          </h3>
          <div class="grid grid-cols-3 gap-1">
            <button
              v-for="opt in groupOptions"
              :key="opt.value"
              type="button"
              class="text-[11px] px-2 py-1 rounded transition-colors text-center"
              :class="groupBy === opt.value
                ? 'bg-accent/10 text-accent font-medium'
                : 'text-secondary hover:bg-surface-hover'"
              :aria-pressed="groupBy === opt.value"
              @click.stop="emit('set-group-by', opt.value)"
            >{{ opt.label }}</button>
          </div>
        </section>

        <!-- Density -->
        <section class="px-3 pb-2 border-t border-subtle pt-2">
          <h3 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary mb-1.5">
            Density
          </h3>
          <div
            class="inline-flex w-full items-center rounded-md border border-subtle overflow-hidden"
            role="group"
            aria-label="Row density"
          >
            <button
              v-for="opt in densityOptions"
              :key="opt.value"
              type="button"
              class="flex-1 text-[11px] px-2 py-1 transition-colors"
              :class="density === opt.value
                ? 'bg-accent/10 text-accent font-medium'
                : 'text-secondary hover:bg-surface-hover'"
              :aria-pressed="density === opt.value"
              @click.stop="emit('set-density', opt.value)"
            >{{ opt.label }}</button>
          </div>
        </section>

        <!-- Properties -->
        <section class="border-t border-subtle pt-2">
          <h3 class="text-[10px] uppercase tracking-wide font-semibold text-tertiary px-3 mb-1">
            Properties
          </h3>
          <div class="max-h-[20rem] overflow-y-auto">
            <button
              v-for="col in TICKET_COLUMNS"
              :key="col.id"
              type="button"
              role="menuitemcheckbox"
              :aria-checked="isOn(col)"
              :disabled="col.id === 'title'"
              class="w-full px-3 py-1.5 flex items-start gap-2 text-left hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              @click.stop="emit('toggle-column', col.id)"
            >
              <span
                class="w-3.5 h-3.5 rounded border flex items-center justify-center shrink-0 mt-0.5"
                :class="isOn(col) ? 'bg-accent border-accent' : 'border-default'"
              >
                <Icon
                  v-if="isOn(col)"
                  name="check"
                  class="w-2.5 h-2.5 text-on-accent"
                />
              </span>
              <span class="flex-1 min-w-0">
                <span class="block text-xs text-primary">
                  {{ col.label === '#' ? 'Ticket #' : col.label }}
                </span>
                <span class="block text-[10px] text-tertiary truncate">
                  {{ col.description }}
                </span>
              </span>
            </button>
          </div>
        </section>
      </div>

      <footer class="border-t border-subtle px-3 py-2 flex items-center justify-between gap-2">
        <button
          type="button"
          class="text-[11px] text-tertiary hover:text-primary"
          @click="emit('reset')"
        >Reset to view</button>
        <button
          v-if="canSaveToView"
          type="button"
          class="text-[11px] font-medium px-2 py-1 rounded bg-accent text-on-accent disabled:opacity-50 disabled:cursor-not-allowed"
          :disabled="!layoutDirty"
          @click="emit('save')"
        >Save to view</button>
      </footer>
    </ResponsiveMenu>
  </div>
</template>
