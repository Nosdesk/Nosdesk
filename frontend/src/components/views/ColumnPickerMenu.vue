<script setup lang="ts">
/**
 * Column visibility picker. Composes the canonical menu
 * primitives — the same `<MenuList>` that powers context menus
 * and view-switcher rows. Each column toggles via a row with
 * a leading checkbox; the menu stays open between toggles
 * (Linear / Asana convention) so a multi-column adjustment
 * doesn't require reopening the popover for each pick.
 *
 * The popover knows nothing about persistence; the parent
 * decides whether to save the choice to localStorage, the
 * saved view's shape, or both.
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

const props = defineProps<{
  visible: ColumnId[]
  /** True when the active view is editable (saved view owned by
   * the user / workspace) — surfaces the "Save layout to view"
   * affordance below the toggles. */
  canSaveToView?: boolean
  /** True when the local choice differs from the saved view's
   * canonical column set; drives the Save button enabled state. */
  layoutDirty?: boolean
}>()

const emit = defineEmits<{
  (e: 'toggle', id: ColumnId): void
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
</script>

<template>
  <div class="inline-flex">
    <button
      ref="triggerRef"
      type="button"
      class="inline-flex items-center gap-1.5 text-[11px] text-secondary hover:text-primary hover:bg-surface-hover px-2 py-1 rounded-md transition-colors"
      :aria-expanded="open"
      aria-haspopup="menu"
      title="Columns"
      @click="open = !open"
    >
      <Icon name="settings" class="w-3.5 h-3.5" />
      Columns
    </button>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      title="Columns"
      placement="bottom-end"
      react-to-scroll="reposition"
      :offset="4"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[16rem] max-w-[20rem]"
      @close="open = false"
    >
      <div class="max-h-[24rem] overflow-y-auto py-1">
        <button
          v-for="col in TICKET_COLUMNS"
          :key="col.id"
          type="button"
          role="menuitemcheckbox"
          :aria-checked="isOn(col)"
          :disabled="col.id === 'title'"
          class="w-full px-3 py-1.5 flex items-start gap-2 text-left hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          @click.stop="emit('toggle', col.id)"
        >
          <span
            class="w-3.5 h-3.5 rounded border flex items-center justify-center shrink-0 mt-0.5"
            :class="isOn(col)
              ? 'bg-accent border-accent'
              : 'border-default'"
          >
            <Icon
              v-if="isOn(col)"
              name="check"
              class="w-2.5 h-2.5 text-on-accent"
            />
          </span>
          <span class="flex-1 min-w-0">
            <span class="block text-xs text-primary">{{ col.label === '#' ? 'Ticket #' : col.label }}</span>
            <span class="block text-[10px] text-tertiary truncate">{{ col.description }}</span>
          </span>
        </button>
      </div>
      <footer class="border-t border-subtle px-3 py-2 flex items-center justify-between gap-2">
        <button
          type="button"
          class="text-[11px] text-tertiary hover:text-primary"
          @click="emit('reset')"
        >Reset</button>
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
