<script setup lang="ts">
/**
 * Three-dot actions menu for the project detail header. Chrome
 * (positioning, dismiss, focus, dividers, danger styling) lives in
 * `<Popover>`/`<ResponsiveMenu>` + `<MenuList>`; this is just the
 * project-specific wiring. The parent owns the side effects (rename
 * goes to inline edit, status flips the sync row, delete confirms in
 * a modal) and listens via per-action emits.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import { buildProjectMenuItems } from '@/utils/projectMenuItems'

const props = defineProps<{ status: string }>()

const emit = defineEmits<{
  (e: 'set-status', status: string): void
  (e: 'delete'): void
}>()

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

// Rename is omitted here: in the project header the title is edited
// directly in the main site header. (The projects list keeps rename via
// its own context menu.)
const menuItems = computed<MenuItem[]>(() =>
  buildProjectMenuItems(props.status, t).filter((i) => i.id !== 'rename'),
)

function toggle() {
  isOpen.value = !isOpen.value
}

function handleSelect(id: string) {
  if (id === 'delete') emit('delete')
  else if (id.startsWith('status:')) emit('set-status', id.slice('status:'.length))
  isOpen.value = false
}
</script>

<template>
  <div class="relative">
    <button
      ref="triggerRef"
      type="button"
      class="p-1.5 rounded-md hover:bg-surface-hover transition-colors text-secondary hover:text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-accent inline-flex items-center justify-center min-h-[44px] min-w-[44px] sm:min-h-0 sm:min-w-0"
      :class="{ 'bg-surface-hover text-primary': isOpen }"
      :title="t('project-actions-menu-trigger')"
      :aria-label="t('project-actions-menu-trigger')"
      @click="toggle"
    >
      <Icon name="more" size="md" />
    </button>

    <ResponsiveMenu
      :open="isOpen"
      :anchor="anchor"
      :title="t('project-actions-menu-trigger')"
      placement="bottom-end"
      react-to-scroll="reposition"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[200px]"
      @close="isOpen = false"
    >
      <MenuList :items="menuItems" @select="handleSelect" />
    </ResponsiveMenu>
  </div>
</template>
