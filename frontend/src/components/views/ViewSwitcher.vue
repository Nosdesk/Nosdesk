<script setup lang="ts">
/**
 * Linear-style saved-view switcher. The active view's name acts
 * as a button; clicking opens a popover listing every available
 * view grouped by source (built-in vs saved). The pattern
 * scales to N saved views without pushing growth into the
 * header bar.
 *
 * Two design notes worth keeping:
 *
 * - The trigger shows only the active view name + a chevron.
 *   The page title above it (`Tickets`) carries the route
 *   identity; the switcher tells the user *which* view they're
 *   looking at, not what page they're on.
 *
 * - Per-view edit actions (rename, archive) live inside the
 *   popover next to each row, not in the header. That keeps the
 *   header stable as the saved-view set grows.
 */
import { computed, ref } from 'vue'
import Popover from '@/components/common/Popover.vue'
import Icon from '@/components/common/Icon.vue'
import type { PopoverAnchor } from '@/composables/usePopover'

export interface ViewSwitcherItem {
  id: string
  name: string
  /** Optional secondary text (e.g. "Workspace view"). */
  hint?: string
  /** Optional grouping label; consecutive items with the same
   * group render under a shared subheader. */
  group?: string
  /** When true, the item shows rename / archive affordances on
   * hover. Built-in views set this to false. */
  editable?: boolean
}

const props = defineProps<{
  items: ViewSwitcherItem[]
  activeId: string
}>()

const emit = defineEmits<{
  (e: 'select', id: string): void
  (e: 'rename', id: string): void
  (e: 'archive', id: string): void
}>()

const triggerRef = ref<HTMLElement | null>(null)
const open = ref(false)

// Live anchor accessor — popover repositions if the trigger
// element re-mounts (cf. composables/usePopover.ts).
const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerRef.value,
}))

const active = computed<ViewSwitcherItem | undefined>(() =>
  props.items.find((i) => i.id === props.activeId),
)

interface Group {
  label: string | null
  items: ViewSwitcherItem[]
}

const grouped = computed<Group[]>(() => {
  const groups = new Map<string, ViewSwitcherItem[]>()
  const order: string[] = []
  for (const item of props.items) {
    const key = item.group ?? ''
    if (!groups.has(key)) {
      groups.set(key, [])
      order.push(key)
    }
    groups.get(key)!.push(item)
  }
  return order.map((key) => ({
    label: key === '' ? null : key,
    items: groups.get(key) ?? [],
  }))
})

function pick(id: string): void {
  emit('select', id)
  open.value = false
}
</script>

<template>
  <div class="inline-flex">
    <button
      ref="triggerRef"
      type="button"
      class="inline-flex items-center gap-1.5 text-sm font-medium text-primary hover:bg-surface-hover rounded-md px-2 py-1 -ml-2 transition-colors"
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="open = !open"
    >
      <span>{{ active?.name ?? 'View' }}</span>
      <Icon name="chevronDown" class="w-3.5 h-3.5 text-tertiary" />
    </button>

    <Popover
      :open="open"
      :anchor="anchor"
      placement="bottom-start"
      role="menu"
      popover-class="bg-app border border-default rounded-md shadow-lg py-1 min-w-[16rem] max-w-[20rem]"
      :auto-focus="false"
      @close="open = false"
    >
      <template v-for="(group, gi) in grouped" :key="gi">
        <div
          v-if="group.label"
          class="text-[10px] uppercase tracking-wide font-semibold text-tertiary px-3 py-1.5"
        >{{ group.label }}</div>
        <ul class="flex flex-col">
          <li
            v-for="item in group.items"
            :key="item.id"
            class="group/row flex items-center gap-2 px-2"
          >
            <button
              type="button"
              class="flex-1 text-left flex items-center gap-2 px-2 py-1.5 rounded text-sm hover:bg-surface-hover transition-colors"
              :class="item.id === activeId ? 'text-primary font-medium' : 'text-secondary'"
              @click="pick(item.id)"
            >
              <Icon
                v-if="item.id === activeId"
                name="check"
                class="w-3.5 h-3.5 text-accent"
              />
              <span v-else class="w-3.5 inline-block" aria-hidden="true" />
              <span class="flex-1 truncate">{{ item.name }}</span>
              <span v-if="item.hint" class="text-[11px] text-tertiary">{{ item.hint }}</span>
            </button>
            <div
              v-if="item.editable"
              class="opacity-0 group-hover/row:opacity-100 transition-opacity flex items-center gap-0.5 pr-1"
            >
              <button
                type="button"
                class="text-[11px] text-tertiary hover:text-primary px-1.5 py-1 rounded hover:bg-surface-hover"
                @click.stop="$emit('rename', item.id)"
              >Rename</button>
              <button
                type="button"
                class="text-[11px] text-tertiary hover:text-rose-600 px-1.5 py-1 rounded hover:bg-surface-hover"
                @click.stop="$emit('archive', item.id)"
              >Archive</button>
            </div>
          </li>
        </ul>
        <div
          v-if="gi < grouped.length - 1"
          class="border-t border-subtle my-1"
          aria-hidden="true"
        />
      </template>
    </Popover>
  </div>
</template>
