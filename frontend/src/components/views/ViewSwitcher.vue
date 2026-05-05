<script setup lang="ts">
/**
 * Saved-view switcher. Composes the canonical menu primitives:
 * `<ResponsiveMenu>` for the popover/sheet shell, `<MenuList>`
 * for the rows. Built-in views, workspace / project / private
 * groups land as `MenuItem` headings + checked rows so the
 * active view reads with the same vocabulary the rest of the
 * app's menus use (commands palette, dashboard widget menus,
 * etc.).
 *
 * Per-view edit affordances (rename / archive) live as menu
 * items prefixed by the view name when `editable` is set —
 * exposes them without a custom hover-row pattern that wouldn't
 * survive on touch devices.
 */
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import type { PopoverAnchor } from '@/composables/usePopover'

export interface ViewSwitcherItem {
  id: string
  name: string
  /** Optional grouping label; consecutive items with the same
   * group key render under a shared heading. */
  group?: string
  /** When true, rename / archive entries surface in the menu
   * tail. Built-in views set this false. */
  editable?: boolean
}

const props = withDefaults(defineProps<{
  items: ViewSwitcherItem[]
  activeId: string
  /** 'sm' is the original toolbar trigger; 'lg' renders as a
   * page-title button (text-lg + larger chevron) so the same
   * component can serve both the small inline switcher and the
   * dominant header title on the tickets list. */
  size?: 'sm' | 'lg'
}>(), {
  size: 'sm',
})

const emit = defineEmits<{
  (e: 'select', id: string): void
  (e: 'rename', id: string): void
  (e: 'archive', id: string): void
}>()

const triggerRef = ref<HTMLElement | null>(null)
const open = ref(false)

const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerRef.value,
}))

const active = computed<ViewSwitcherItem | undefined>(() =>
  props.items.find((i) => i.id === props.activeId),
)

/** Flatten the grouped item list into the MenuItem array
 * MenuList expects, inserting headings as group changes and
 * trailing rename / archive entries for editable rows under
 * a divider. */
const menuItems = computed<MenuItem[]>(() => {
  const out: MenuItem[] = []
  let lastGroup: string | undefined
  for (const item of props.items) {
    if (item.group && item.group !== lastGroup) {
      out.push({ id: `__group:${item.group}`, label: item.group, heading: true })
      lastGroup = item.group
    }
    out.push({
      id: `select:${item.id}`,
      label: item.name,
      checked: item.id === props.activeId,
    })
  }
  const editableActive = props.items.find(
    (i) => i.id === props.activeId && i.editable,
  )
  if (editableActive) {
    out.push({ id: `__rename:${editableActive.id}`, label: `Rename "${editableActive.name}"`, divider: true })
    out.push({ id: `__archive:${editableActive.id}`, label: `Archive "${editableActive.name}"`, danger: true })
  }
  return out
})

function handleSelect(id: string): void {
  if (id.startsWith('select:')) {
    emit('select', id.slice('select:'.length))
    open.value = false
    return
  }
  if (id.startsWith('__rename:')) {
    emit('rename', id.slice('__rename:'.length))
    open.value = false
    return
  }
  if (id.startsWith('__archive:')) {
    emit('archive', id.slice('__archive:'.length))
    open.value = false
    return
  }
  // Heading clicks (`__group:*`) are no-ops by design.
}
</script>

<template>
  <div class="inline-flex">
    <button
      ref="triggerRef"
      type="button"
      class="inline-flex items-center text-primary rounded-md transition-colors"
      :class="[
        open ? 'bg-surface-hover' : 'hover:bg-surface-hover',
        props.size === 'lg'
          ? 'gap-2 text-lg font-semibold px-2 py-1 -ml-2'
          : 'gap-1.5 text-sm font-medium px-2 py-1 -ml-2',
      ]"
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="open = !open"
    >
      <span>{{ active?.name ?? 'View' }}</span>
      <Icon
        name="chevronDown"
        class="text-tertiary"
        :class="props.size === 'lg' ? 'w-4 h-4' : 'w-3.5 h-3.5'"
      />
    </button>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      :title="active?.name ?? 'Views'"
      placement="bottom-start"
      react-to-scroll="reposition"
      :offset="4"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[14rem] max-w-[20rem]"
      @close="open = false"
    >
      <div class="py-1 max-h-[28rem] overflow-y-auto">
        <MenuList :items="menuItems" @select="handleSelect" />
      </div>
    </ResponsiveMenu>
  </div>
</template>
