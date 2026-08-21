<script setup lang="ts">
/**
 * Sidebar workspace switcher. Lists the signed-in user's
 * memberships from `/api/me/workspaces` and navigates to the
 * selected tenant's host (custom domain or slug subdomain).
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import Icon from '@/components/common/Icon.vue';
import { initialsFrom, monogramColor, monogramDataUri } from '@/utils/monogram';
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
    // `checked` takes the icon gutter for the active row, which is the more
    // useful signal there; the trigger already shows that workspace's mark.
    iconUrl: ws.logo_url ?? monogramDataUri(ws.name, ws.workspace_uuid),
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

/** Mark for the active workspace: its logo, else a monogram. */
const activeMark = computed(() => {
  const active = store.activeWorkspace;
  if (!active) return null;
  return {
    logo: active.logo_url,
    initials: initialsFrom(active.name),
    color: monogramColor(active.workspace_uuid),
  };
});

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
      :aria-label="
        store.activeWorkspace
          ? t('nav-workspace-switcher-current', { name: store.activeWorkspace.name })
          : t('nav-workspace-switcher-label')
      "
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="open = !open"
    >
      <div class="flex items-center gap-2 min-w-0" :class="collapsed ? '' : 'flex-1'">
        <!-- Decorative: the workspace name sits in the label beside it, and in
             the button's aria-label when the sidebar is collapsed. -->
        <img
          v-if="activeMark?.logo"
          :src="activeMark.logo"
          alt=""
          class="h-5 w-5 shrink-0 rounded-[0.3125rem] object-cover"
        />
        <span
          v-else-if="activeMark"
          class="h-5 w-5 shrink-0 rounded-[0.3125rem] grid place-items-center text-[0.625rem] font-semibold leading-none text-white"
          :style="{ backgroundColor: activeMark.color }"
          aria-hidden="true"
          >{{ activeMark.initials }}</span
        >
        <Icon v-else name="folder" class="shrink-0" />
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
