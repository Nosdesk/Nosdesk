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

const props = defineProps<{ status: string }>()

const emit = defineEmits<{
  (e: 'rename'): void
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

const STATUSES = ['active', 'completed', 'archived'] as const

const menuItems = computed<MenuItem[]>(() => [
  { id: 'rename', label: t('project-actions-rename') },
  ...STATUSES.map((s, i) => ({
    id: `status:${s}`,
    label: t(`project-actions-status-${s}`),
    check: props.status === s,
    divider: i === 0,
  })),
  { id: 'delete', label: t('project-actions-delete'), danger: true, divider: true },
])

function toggle() {
  isOpen.value = !isOpen.value
}

function handleSelect(id: string) {
  if (id === 'rename') emit('rename')
  else if (id === 'delete') emit('delete')
  else if (id.startsWith('status:')) emit('set-status', id.slice('status:'.length))
  isOpen.value = false
}
</script>

<template>
  <div class="relative">
    <button
      ref="triggerRef"
      type="button"
      class="p-1.5 rounded-md hover:bg-surface-hover transition-colors text-secondary hover:text-primary"
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
