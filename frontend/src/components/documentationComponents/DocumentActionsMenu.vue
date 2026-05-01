<script setup lang="ts">
/**
 * Three-dot dropdown for the documentation page header. The
 * actual menu chrome (positioning, dismiss, focus, gutter-
 * aligned items, dividers, danger styling) lives in
 * `<Popover>` + `<MenuList>` — this file is just the
 * domain-specific wiring: which actions belong here, how their
 * labels flip with state, and the two-stage delete confirm.
 *
 * Action items are derived reactively from props so toggle
 * states (Subscribe/Unsubscribe, Archive/Unarchive,
 * Publish/Unpublish, confirming-trash) update without manual
 * mutation. The parent owns the actual side effects and listens
 * via per-action emits.
 */
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import { ICON_REGISTRY } from '@/components/common/icons'

const props = defineProps<{
  pageId: number | string
  pageTitle: string
  pageSlug?: string
  pageStatus?: string
  showPermissions?: boolean
  isSubscribed?: boolean
}>()

const emit = defineEmits<{
  (e: 'delete'): void
  (e: 'duplicate'): void
  (e: 'archive'): void
  (e: 'restore'): void
  (e: 'publish'): void
  (e: 'unpublish'): void
  (e: 'move'): void
  (e: 'export'): void
  (e: 'collections'): void
  (e: 'permissions'): void
  (e: 'subscribe'): void
  (e: 'unsubscribe'): void
  (e: 'insights'): void
  (e: 'history'): void
}>()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const confirmingDelete = ref(false)

const isArchived = computed(() => props.pageStatus === 'archived')
const isPublished = computed(() => props.pageStatus === 'published')

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

function toggle() {
  if (isOpen.value) {
    closeMenu()
  } else {
    isOpen.value = true
    confirmingDelete.value = false
  }
}

function closeMenu() {
  isOpen.value = false
  confirmingDelete.value = false
}

/**
 * Action items in render order. Reactive on every prop change so
 * Subscribe ↔ Unsubscribe, Archive ↔ Unarchive, Publish ↔
 * Unpublish, "Move to Trash" ↔ "Confirm trash?" all flip
 * without manual mutation. Iconography follows the central
 * registry; items where the previous glyph was a weak metaphor
 * (Duplicate, Collections, Publish) intentionally go label-only
 * and keep the gutter aligned via MenuList's fixed-width column.
 */
const menuItems = computed<MenuItem[]>(() => {
  const items: MenuItem[] = [
    {
      id: 'subscription',
      label: props.isSubscribed ? 'Unsubscribe' : 'Subscribe',
      icon: ICON_REGISTRY.bell.d,
      active: props.isSubscribed,
    },
    { id: 'insights', label: 'Insights', icon: ICON_REGISTRY.insights.d, divider: true },
    { id: 'history', label: 'Revision history', icon: ICON_REGISTRY.history.d },
    { id: 'print', label: 'Print', icon: ICON_REGISTRY.print.d, divider: true },
    { id: 'duplicate', label: 'Duplicate' },
    { id: 'export', label: 'Download Markdown', icon: ICON_REGISTRY.download.d },
    { id: 'move', label: 'Move to...', icon: ICON_REGISTRY.move.d },
    { id: 'collections', label: 'Collections', divider: true },
    {
      id: 'archive',
      label: isArchived.value ? 'Unarchive' : 'Archive',
      icon: ICON_REGISTRY.archive.d,
    },
  ]
  if (props.showPermissions) {
    items.push({
      id: 'permissions',
      label: 'Permissions',
      icon: ICON_REGISTRY.lock.d,
    })
  }
  items.push({ id: 'publish', label: isPublished.value ? 'Unpublish' : 'Publish' })
  items.push({
    id: 'delete',
    label: confirmingDelete.value ? 'Confirm trash?' : 'Move to Trash',
    icon: ICON_REGISTRY.trash.d,
    danger: true,
    divider: true,
  })
  return items
})

/**
 * Map a menu-item id to its emit. Two items keep the menu open
 * after firing: subscription (silent toggle, no follow-up UI)
 * and the first delete click (turns the row into "Confirm
 * trash?"). Everything else closes after dispatch.
 */
function handleSelect(id: string) {
  switch (id) {
    case 'subscription':
      if (props.isSubscribed) emit('unsubscribe')
      else emit('subscribe')
      closeMenu()
      return
    case 'insights':
      emit('insights')
      break
    case 'history':
      emit('history')
      break
    case 'print':
      window.print()
      break
    case 'duplicate':
      emit('duplicate')
      break
    case 'export':
      emit('export')
      break
    case 'move':
      emit('move')
      break
    case 'collections':
      emit('collections')
      break
    case 'archive':
      if (isArchived.value) emit('restore')
      else emit('archive')
      break
    case 'permissions':
      emit('permissions')
      break
    case 'publish':
      if (isPublished.value) emit('unpublish')
      else emit('publish')
      break
    case 'delete':
      // Two-stage destructive confirmation. First click flips
      // the label to "Confirm trash?"; second click commits.
      // The menu stays open between the two clicks so the user
      // can see the label change.
      if (!confirmingDelete.value) {
        confirmingDelete.value = true
        return
      }
      emit('delete')
      break
  }
  closeMenu()
}
</script>

<template>
  <div class="relative">
    <button
      ref="triggerRef"
      @click="toggle"
      class="p-1.5 rounded-md hover:bg-surface-hover transition-colors text-secondary hover:text-primary"
      :class="{ 'bg-surface-hover text-primary': isOpen }"
      title="Page actions"
      aria-label="Page actions"
    >
      <Icon name="more" size="md" />
    </button>

    <ResponsiveMenu
      :open="isOpen"
      :anchor="anchor"
      title="Page actions"
      placement="bottom-end"
      react-to-scroll="reposition"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[208px]"
      @close="closeMenu"
    >
      <MenuList :items="menuItems" @select="handleSelect" />
    </ResponsiveMenu>
  </div>
</template>
