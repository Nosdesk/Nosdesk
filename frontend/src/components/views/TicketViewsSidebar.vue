<script setup lang="ts">
/**
 * Persistent secondary sidebar listing every ticket view the user
 * can switch into. Pattern follows Linear's Issues sidebar:
 * the view list is always visible, click switches instantly, no
 * popover hop. Saved views grouped by scope so workspace-shared
 * views stay distinct from a user's private ones.
 *
 * The bar is collapsible — at narrow viewports the icon-only
 * spine keeps the table its full width while still surfacing the
 * active view name in the toolbar above. Density-conscious by
 * default: row height matches the table density toggle so the
 * eye doesn't shift when scanning between sidebar and rows.
 */
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'

export interface TicketViewItem {
  id: string
  name: string
  /** Group label; consecutive items with the same group share a
   * non-interactive subheader. */
  group?: string
  /** Surface rename / archive on hover when the active view is
   * editable. Built-in views set this to false. */
  editable?: boolean
  /** Optional badge — e.g. count of matching tickets. */
  count?: number
}

const props = defineProps<{
  items: TicketViewItem[]
  activeId: string
}>()

const emit = defineEmits<{
  (e: 'select', id: string): void
  (e: 'rename', id: string): void
  (e: 'archive', id: string): void
  (e: 'save'): void
}>()

const collapsed = ref<boolean>(loadCollapsed())

function loadCollapsed(): boolean {
  if (typeof localStorage === 'undefined') return false
  return localStorage.getItem('ticket-views-sidebar-collapsed') === '1'
}

function toggleCollapsed(): void {
  collapsed.value = !collapsed.value
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(
      'ticket-views-sidebar-collapsed',
      collapsed.value ? '1' : '0',
    )
  }
}

interface Group {
  label: string | null
  items: TicketViewItem[]
}

const groups = computed<Group[]>(() => {
  const map = new Map<string, TicketViewItem[]>()
  const order: string[] = []
  for (const item of props.items) {
    const key = item.group ?? ''
    if (!map.has(key)) {
      map.set(key, [])
      order.push(key)
    }
    map.get(key)!.push(item)
  }
  return order.map((key) => ({
    label: key === '' ? null : key,
    items: map.get(key) ?? [],
  }))
})

function pick(item: TicketViewItem): void {
  emit('select', item.id)
}
</script>

<template>
  <aside
    class="flex flex-col bg-surface border-r border-subtle h-full transition-[width] duration-150"
    :class="collapsed ? 'w-12' : 'w-56'"
    :aria-label="collapsed ? 'Views (collapsed)' : 'Views'"
  >
    <header
      class="flex items-center justify-between h-9 px-2 border-b border-subtle shrink-0"
    >
      <span
        v-if="!collapsed"
        class="text-[11px] uppercase tracking-wide font-semibold text-tertiary px-1"
      >Views</span>
      <button
        type="button"
        class="text-tertiary hover:text-primary p-1 rounded hover:bg-surface-hover transition-colors"
        :title="collapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        :aria-label="collapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        @click="toggleCollapsed"
      >
        <Icon
          name="chevronDown"
          class="w-3.5 h-3.5 transition-transform"
          :class="collapsed ? '-rotate-90' : 'rotate-90'"
        />
      </button>
    </header>

    <div class="flex-1 overflow-y-auto py-1">
      <template v-for="(group, gi) in groups" :key="gi">
        <h3
          v-if="!collapsed && group.label"
          class="text-[10px] uppercase tracking-wide font-semibold text-tertiary px-3 pt-2 pb-1"
        >{{ group.label }}</h3>
        <ul role="list" class="flex flex-col">
          <li
            v-for="item in group.items"
            :key="item.id"
            class="group/row"
          >
            <button
              type="button"
              class="w-full flex items-center gap-2 px-2 py-1.5 text-sm text-left rounded-md transition-colors mx-1 my-0.5"
              :class="item.id === activeId
                ? 'bg-accent/10 text-accent font-medium'
                : 'text-secondary hover:text-primary hover:bg-surface-hover'"
              :title="collapsed ? item.name : undefined"
              @click="pick(item)"
            >
              <span
                v-if="!collapsed"
                class="flex-1 truncate"
              >{{ item.name }}</span>
              <span v-else class="w-full flex justify-center">
                {{ item.name.slice(0, 1).toUpperCase() }}
              </span>
              <span
                v-if="!collapsed && item.count != null"
                class="text-[11px] text-tertiary tabular-nums"
              >{{ item.count }}</span>
            </button>
          </li>
        </ul>
      </template>
    </div>

    <footer
      v-if="!collapsed"
      class="border-t border-subtle p-2 shrink-0"
    >
      <button
        type="button"
        class="w-full text-left flex items-center gap-2 px-2 py-1.5 text-xs font-medium text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
        @click="emit('save')"
      >
        <Icon name="add" class="w-3.5 h-3.5" />
        Save current as view
      </button>
    </footer>
  </aside>
</template>
