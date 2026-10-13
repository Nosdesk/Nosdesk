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
import MenuItem from '@/components/common/MenuItem.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import type { PopoverAnchor } from '@/composables/popoverAnchor'
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

// Sectioned columns are optional extras (asset kind attributes), so they
// don't count towards "columns tucked away".
const hiddenCount = computed<number>(
  () => props.columns.filter((c) => !c.pickerSection && props.isHidden(c.field)).length,
)

/** Core columns first, then each picker section under its heading. */
const groups = computed(() => {
  const out: { section: string | null; columns: DataTableColumnLike[] }[] = []
  for (const col of props.columns) {
    const section = col.pickerSection ?? null
    let group = out.find((g) => g.section === section)
    if (!group) {
      group = { section, columns: [] }
      if (section === null) out.unshift(group)
      else out.push(group)
    }
    group.columns.push(col)
  }
  return out
})
</script>

<template>
  <div class="inline-flex">
    <button
      ref="triggerRef"
      type="button"
      class="inline-flex items-center gap-1 text-2xs px-2 h-6 rounded-md border transition-colors text-tertiary hover:text-primary border-subtle hover:border-default hover:bg-surface-hover"
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="open = !open"
    >
      <span>{{ triggerLabel ?? $t('views-column-picker-trigger') }}</span>
      <!-- Subtle hidden-count badge so the user can tell at a
           glance whether columns are tucked away. -->
      <span
        v-if="hiddenCount > 0"
        class="text-3xs text-tertiary"
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
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[14rem] max-w-[calc(100vw-1rem)] sm:max-w-[20rem]"
      @close="open = false"
    >
      <div class="py-1 max-h-[20rem] overflow-y-auto">
        <template v-for="group in groups" :key="group.section ?? ''">
        <p
          v-if="group.section"
          class="px-3 pt-2 pb-1 text-3xs font-medium uppercase tracking-wide text-tertiary border-t border-subtle mt-1"
        >{{ group.section }}</p>
        <MenuItem
          v-for="col in group.columns"
          :key="col.field"
          role="menuitemcheckbox"
          :checked="!isHidden(col.field)"
          :disabled="isPinned(col.field)"
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
          <span class="flex-1 text-primary truncate">{{ col.label }}</span>
        </MenuItem>
        </template>
      </div>
      <footer class="border-t border-subtle px-3 py-1.5 flex items-center justify-end">
        <button
          type="button"
          class="text-2xs text-tertiary hover:text-primary"
          @click="emit('reset'); open = false"
        >{{ $t('views-column-picker-reset') }}</button>
      </footer>
    </ResponsiveMenu>
  </div>
</template>
