<script setup lang="ts">
/**
 * Column visibility picker for the shared DataTable. Sits in
 * the filters row next to GroupByMenu / SaveViewModal trigger;
 * shows a checkbox-per-column popover so the user can switch
 * columns on and off without leaving the list. Reorder happens
 * via header drag in the table itself.
 *
 * Pair with `useDataTableColumns` — the composable owns the
 * persistence and ordering; this component is the picker UI
 * over it.
 */
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import type { PopoverAnchor } from '@/composables/usePopover'
import type { DataTableColumnLike } from '@/composables/useDataTableColumns'

const props = defineProps<{
  columns: readonly DataTableColumnLike[]
  isHidden: (field: string) => boolean
  isPinned: (field: string) => boolean
  triggerLabel?: string
}>()

const emit = defineEmits<{
  (e: 'toggle', field: string): void
  (e: 'reset'): void
}>()

const triggerRef = ref<HTMLElement | null>(null)
const open = ref(false)

const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerRef.value,
}))

const hiddenCount = computed<number>(
  () => props.columns.filter((c) => props.isHidden(c.field)).length,
)
</script>

<template>
  <div class="inline-flex">
    <button
      ref="triggerRef"
      type="button"
      class="inline-flex items-center gap-1 text-[11px] px-2 h-6 rounded-md border transition-colors text-tertiary hover:text-primary border-subtle hover:border-default hover:bg-surface-hover"
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="open = !open"
    >
      <span>{{ triggerLabel ?? $t('views-column-picker-trigger') }}</span>
      <!-- Subtle hidden-count badge so the user can tell at a
           glance whether columns are tucked away. -->
      <span
        v-if="hiddenCount > 0"
        class="text-[10px] text-tertiary"
      >{{ hiddenCount }}</span>
      <Icon name="chevronDown" class="w-3 h-3 opacity-70" />
    </button>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      :title="$t('views-column-picker-trigger')"
      placement="bottom-start"
      react-to-scroll="reposition"
      :offset="4"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[14rem] max-w-[20rem]"
      @close="open = false"
    >
      <div class="py-1 max-h-[20rem] overflow-y-auto">
        <button
          v-for="col in columns"
          :key="col.field"
          type="button"
          role="menuitemcheckbox"
          :aria-checked="!isHidden(col.field)"
          :disabled="isPinned(col.field)"
          class="w-full px-3 py-1.5 grid grid-cols-[auto_1fr] items-center gap-x-2 text-left transition-colors duration-75 hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed"
          @click="emit('toggle', col.field)"
        >
          <span
            class="w-3.5 h-3.5 rounded border flex items-center justify-center shrink-0 transition-colors duration-75"
            :class="!isHidden(col.field) ? 'bg-accent border-accent' : 'border-default'"
          >
            <Icon
              v-if="!isHidden(col.field)"
              name="check"
              class="w-2.5 h-2.5 text-on-accent"
            />
          </span>
          <span class="text-xs text-primary truncate">{{ col.label }}</span>
        </button>
      </div>
      <footer class="border-t border-subtle px-3 py-1.5 flex items-center justify-end">
        <button
          type="button"
          class="text-[11px] text-tertiary hover:text-primary"
          @click="emit('reset'); open = false"
        >{{ $t('views-column-picker-reset') }}</button>
      </footer>
    </ResponsiveMenu>
  </div>
</template>
