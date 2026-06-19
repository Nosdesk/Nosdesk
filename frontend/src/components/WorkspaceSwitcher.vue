<script setup lang="ts">
/**
 * Sidebar workspace switcher. Lists the signed-in user's
 * memberships from `/api/me/workspaces` and navigates to the
 * selected tenant's host (custom domain or slug subdomain).
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue';
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue';
import Spinner from '@/components/common/Spinner.vue';
import type { PopoverAnchor } from '@/composables/usePopover';
import { useMyWorkspacesStore } from '@/stores/myWorkspaces';
import { useWorkspaceSwitch } from '@/composables/useWorkspaceSwitch';

withDefaults(
  defineProps<{
    /** Icon-only trigger for the collapsed sidebar. */
    collapsed?: boolean;
  }>(),
  { collapsed: false },
);

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const store = useMyWorkspacesStore();
const { switchWorkspace } = useWorkspaceSwitch();

const open = ref(false);
const triggerRef = ref<HTMLElement | null>(null);

const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerRef.value,
}));

const roleLabel = (role: string) => {
  const key = `admin-workspace-members-role-${role}`;
  const label = t(key);
  return label === key ? role : label;
};

const menuItems = computed<MenuItem[]>(() =>
  store.workspaces.map((ws) => ({
    id: `ws:${ws.workspace_id}`,
    label: ws.name,
    trailing: roleLabel(ws.role),
    checked: ws.workspace_id === store.activeWorkspaceId,
  })),
);

function handleSelect(id: string): void {
  if (!id.startsWith('ws:')) return;
  const workspaceId = Number(id.slice(3));
  const entry = store.workspaces.find((w) => w.workspace_id === workspaceId);
  if (!entry) return;
  if (entry.workspace_id === store.activeWorkspaceId) {
    open.value = false;
    return;
  }
  open.value = false;
  void switchWorkspace(entry);
}

const triggerTitle = computed(() => {
  const active = store.activeWorkspace;
  if (!active) return t('nav-workspace-switcher-label');
  return `${active.name} (${active.slug})`;
});
</script>

<template>
  <div v-if="store.showSwitcher" class="w-full">
    <button
      ref="triggerRef"
      type="button"
      class="w-full rounded-md transition-colors duration-200 flex items-center bg-surface-alt border border-default text-secondary hover:bg-surface-hover hover:text-primary hover:border-subtle"
      :class="
        collapsed
          ? 'px-2 py-1.5 justify-center'
          : 'px-2.5 py-1.5 gap-2 justify-between min-w-0'
      "
      :title="triggerTitle"
      :aria-label="t('nav-workspace-switcher-label')"
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="open = !open"
    >
      <div class="flex items-center gap-2 min-w-0" :class="collapsed ? '' : 'flex-1'">
        <Icon name="folder" class="shrink-0" />
        <template v-if="!collapsed">
          <span v-if="store.isLoading" class="text-sm inline-flex items-center gap-2">
            <Spinner size="xs" :label="$t('nav-workspace-switcher-loading')" />
          </span>
          <span v-else class="text-sm truncate text-primary text-left">
            {{ store.activeWorkspace?.name ?? $t('nav-workspace-switcher-label') }}
          </span>
        </template>
      </div>
      <Icon
        v-if="!collapsed && !store.isLoading"
        name="chevronDown"
        class="text-tertiary shrink-0 w-3.5 h-3.5"
      />
    </button>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      :title="store.activeWorkspace?.name ?? $t('nav-workspace-switcher-label')"
      placement="bottom-start"
      match-anchor-width
      react-to-scroll="reposition"
      :offset="4"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[14rem] max-w-[calc(100vw-1rem)] sm:max-w-[20rem]"
      @close="open = false"
    >
      <div class="py-1 max-h-[28rem] overflow-y-auto">
        <MenuList :items="menuItems" @select="handleSelect" />
      </div>
    </ResponsiveMenu>
  </div>
</template>
