<script setup lang="ts">
/**
 * Role control for a Team-view row. Renders the member's workspace
 * role as a badge; when the caller may change it, the badge is a
 * button that opens a small radio-style menu.
 *
 * Editable and locked rows deliberately render the same badge, so a
 * locked row reads as "same kind of thing, not yours to change" rather
 * than as a different control. The only difference is the chevron and
 * the hover affordance.
 *
 * Which roles the caller may assign is decided by the parent (it knows
 * the caller's tier and the last-owner rule) and passed as
 * `assignable`. The backend re-checks on write, so this is
 * convenience, not the boundary.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Icon from '@/components/common/Icon.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import StatusBadgeCell from '@/components/common/cells/StatusBadgeCell.vue'
import type { WorkspaceMemberRow } from './memberRow'
import { WORKSPACE_ROLES, type WorkspaceRole } from '@nosdesk/core/types/workspace'

defineOptions({ name: 'MemberRoleMenu' })

const props = withDefaults(
  defineProps<{
    member: WorkspaceMemberRow
    /** Roles this caller may assign. Others render dimmed. */
    assignable: readonly WorkspaceRole[]
    disabled?: boolean
  }>(),
  { disabled: false },
)

const emit = defineEmits<{ (e: 'change', role: WorkspaceRole): void }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string>) => fluent.$t(key, args)

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)

/**
 * The popover is mounted on first open, not with the row. Both the
 * desktop and mobile bodies stay mounted (ListPageLayout toggles them
 * with v-show), and the member list isn't paginated, so an eagerly
 * mounted menu costs two ResponsiveMenu instances per editable member
 * — each with its own matchMedia subscription and scroll-lock watcher.
 * On a large workspace that's hundreds of listeners for the at-most
 * one menu that can be open.
 */
const hasOpened = ref(false)

// Function form keeps the lookup live across re-mounts.
const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

const roleLabel = (role: WorkspaceRole) => t(`admin-workspace-members-role-${role}`)

// Every role is listed, not just the assignable ones, so the menu
// doesn't reshuffle between rows.
const menuItems = computed<MenuItem[]>(() =>
  WORKSPACE_ROLES.map((role) => ({
    id: role,
    label: roleLabel(role),
    checked: role === props.member.role,
    // The current role stays undimmed even when the caller couldn't
    // newly assign it, so the tick never sits on a disabled row.
    disabled: !props.assignable.includes(role) && role !== props.member.role,
  })),
)

function toggle() {
  if (props.disabled) return
  hasOpened.value = true
  isOpen.value = !isOpen.value
}

function handleSelect(id: string) {
  isOpen.value = false
  const next = id as WorkspaceRole
  if (next !== props.member.role) emit('change', next)
}
</script>

<template>
  <div v-if="!member.editable" class="inline-flex" :title="member.lockedHint || undefined">
    <StatusBadgeCell type="role" :value="member.role" :label="roleLabel(member.role)" />
  </div>

  <div v-else class="relative inline-flex">
    <button
      ref="triggerRef"
      type="button"
      class="inline-flex items-center gap-1 rounded-full pr-1 transition-colors hover:bg-surface-hover disabled:opacity-50"
      :class="{ 'bg-surface-hover': isOpen }"
      :aria-label="
        t('workspace-members-role-trigger', {
          name: member.name,
          role: roleLabel(member.role),
        })
      "
      aria-haspopup="menu"
      :aria-expanded="isOpen"
      :disabled="disabled"
      @click.stop="toggle"
    >
      <StatusBadgeCell type="role" :value="member.role" :label="roleLabel(member.role)" />
      <Icon name="chevronDown" size="xs" class="text-tertiary" />
    </button>

    <ResponsiveMenu
      v-if="hasOpened"
      :open="isOpen"
      :anchor="anchor"
      :title="t('admin-workspace-members-col-role')"
      placement="bottom-start"
      react-to-scroll="reposition"
      role="menu"
      :min-width="160"
      popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[160px]"
      @close="isOpen = false"
    >
      <MenuList :items="menuItems" @select="handleSelect" />
    </ResponsiveMenu>
  </div>
</template>
