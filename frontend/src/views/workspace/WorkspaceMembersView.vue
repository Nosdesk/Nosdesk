<script setup lang="ts">
/**
 * Tenant self-serve workspace member management (P1.3).
 *
 * Lists the current workspace's members and lets an owner/admin change
 * roles or remove members. Acts on the caller's own workspace via the
 * context-scoped `/workspace/members` endpoints (no workspace id), so a
 * workspace admin can only ever manage their own team.
 *
 * The UI mirrors the backend's tiered authorization: owners manage every
 * role; admins manage only agents and members and can assign only
 * agent/member. Rows the caller can't manage render read-only. The
 * backend re-checks everything, so this is convenience, not the boundary.
 *
 * Shares the list chrome (ListPageLayout + DataTable + cells) with
 * `UsersListView` so Team reads as the same kind of surface. It does
 * *not* use `useListView`: that shell is built around paginated
 * fetching, chip facets, saved views and URL sync, none of which
 * `GET /api/workspace/members` supports — it returns the whole team in
 * one array, so search and sort are local.
 *
 * The endpoint carries only `user_uuid` per row. Names, emails and
 * avatars come from the sync pool, whose `user` aggregate is grouped by
 * workspace and so already holds every teammate (see
 * `backend/sync-models/user.json`).
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import ConfirmModal from '@/components/common/ConfirmModal.vue';
import DataTable from '@/components/common/DataTable.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import ListPageLayout from '@/components/common/ListPageLayout.vue';
import MemberRoleMenu from '@/components/workspace/MemberRoleMenu.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { DateCell, StatusBadgeCell, UserInfoCell } from '@/components/common/cells';
import type { WorkspaceMemberRow } from '@/components/workspace/memberRow';
import { useMobileSearch } from '@/composables/useMobileSearch';
import { usePageCreateAction } from '@/composables/usePageCreateAction';
import workspacesService from '@nosdesk/core/services/workspacesService';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { useAuthStore } from '@/stores/auth';
import {
  WORKSPACE_ROLES,
  WORKSPACE_ROLE_RANK,
  type WorkspaceMember,
  type WorkspaceRole,
} from '@nosdesk/core/types/workspace';
import type { User } from '@nosdesk/core/types/user';
import {
  isHostedDeploymentRef,
  isRoleExternallyManaged,
} from '@nosdesk/core/services/instanceConfig';
import { openControlPlaneSeats } from '@/services/activeWorkspace';
import { extractErrorMessage } from '@/utils/errors';
import * as syncPool from '@nosdesk/core/sync/pool';

defineOptions({ name: 'WorkspaceMembersView' });

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const router = useRouter();
const auth = useAuthStore();
const toast = useToastStore();

const MEMBERS_KEY = ['workspace-members'] as const;
const queryCache = useQueryCache();

const membersQuery = useQuery({
  key: MEMBERS_KEY,
  query: () => workspacesService.listWorkspaceMembers(),
});

const members = computed<WorkspaceMember[]>(() =>
  Array.isArray(membersQuery.data.value) ? membersQuery.data.value : [],
);
const loadError = computed(() =>
  membersQuery.error.value ? t('admin-workspace-members-error-load') : '',
);

const ownerCount = computed(() => members.value.filter((m) => m.role === 'owner').length);

// The caller's role in this workspace drives which rows are editable.
const myRole = computed<WorkspaceRole>(() => auth.user?.workspace_role ?? 'member');
const myUuid = computed(() => auth.user?.uuid ?? '');

const isSaving = ref(false);
const memberToRemove = ref<WorkspaceMemberRow | null>(null);
const searchQuery = ref('');

// Inviting a teammate is the same flow as creating a user (the create
// form owns the `send_invitation` toggle), so the page action routes
// there rather than duplicating it. Registered as the global create
// action so the button lives in the top bar, matching `/users`.
const navigateToInvite = () => {
  void router.push('/users/new');
};
usePageCreateAction(navigateToInvite);

// Mobile has no room for the desktop toolbar, so search + create move
// into the shared mobile search bar. `useListPage` does this for
// paginated views; this view drives it directly.
const mobileSearch = useMobileSearch();
onMounted(() =>
  mobileSearch.registerMobileSearch({
    searchQuery: searchQuery.value,
    placeholder: t('workspace-members-search-placeholder'),
    createIcon: 'user',
    onSearchUpdate: (value: string) => {
      searchQuery.value = value;
    },
    onCreate: navigateToInvite,
  }),
);
onUnmounted(mobileSearch.deregisterMobileSearch);
watch(searchQuery, mobileSearch.updateSearchQuery);

const refresh = () => queryCache.invalidateQueries({ key: MEMBERS_KEY });

/** Owners manage every role; admins only agents/members. */
function canManage(targetRole: WorkspaceRole): boolean {
  if (myRole.value === 'owner') return true;
  if (myRole.value === 'admin') {
    return WORKSPACE_ROLE_RANK[targetRole] < WORKSPACE_ROLE_RANK.admin;
  }
  return false;
}

/** Roles the caller may assign (same tier as canManage). */
const assignableRoles = computed(() => WORKSPACE_ROLES.filter(canManage));

/**
 * Members joined with their pool-resolved identity. The full uuid never
 * takes a column; it lives in the row's title attribute for support
 * lookups. If the `user` aggregate hasn't landed yet the row degrades to
 * a short uuid rather than printing 36 characters where a name goes.
 */
const rows = computed<WorkspaceMemberRow[]>(() =>
  members.value.map((member) => {
    const cached = syncPool.get('user', member.user_uuid) as
      | Pick<User, 'name' | 'email' | 'avatar_url' | 'avatar_thumb'>
      | undefined;
    // Nobody may demote or remove the last owner, whatever their tier.
    const isSoleOwner = member.role === 'owner' && ownerCount.value === 1;
    const status = member.accepted_at ? 'active' : 'pending';
    return {
      id: member.user_uuid,
      user_uuid: member.user_uuid,
      role: member.role,
      invited_at: member.invited_at,
      accepted_at: member.accepted_at,
      name: cached?.name ?? cached?.email ?? member.user_uuid.slice(0, 8),
      email: cached?.email ?? '',
      avatar: cached?.avatar_thumb ?? cached?.avatar_url ?? null,
      isYou: member.user_uuid === myUuid.value,
      status,
      statusLabel:
        status === 'active'
          ? t('workspace-members-status-active')
          : t('admin-workspace-members-accepted-pending'),
      // A staff seat (owner/admin/agent) is control-plane-owned in hosted, so
      // the roster is read-only for those rows; the control-plane hand-off
      // (the header "one door") takes the place of the in-product controls.
      editable:
        canManage(member.role) && !isSoleOwner && !isRoleExternallyManaged(member.role),
      lockedHint: isSoleOwner ? t('admin-workspace-members-last-owner-hint') : '',
    };
  }),
);

// A refetch that fails while rows are on screen keeps the stale list
// rather than blanking the page (see the `error` binding in the
// template), so it's announced here instead. Without this the list
// would go quietly stale with no signal at all.
watch(
  () => membersQuery.error.value,
  (error) => {
    if (error && rows.value.length > 0) toast.error(t('admin-workspace-members-error-load'));
  },
);

// uuid is matched too: it no longer has a column, so pasting one from
// an audit log or support ticket is the only way to find that member.
const filteredRows = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  if (!q) return rows.value;
  return rows.value.filter(
    (row) =>
      row.name.toLowerCase().includes(q) ||
      row.email.toLowerCase().includes(q) ||
      row.user_uuid.toLowerCase().includes(q),
  );
});

// Local sort. Default is role tier descending then name, which puts
// owners and admins at the top where an admin auditing access expects
// them.
const sortField = ref('role');
const sortDirection = ref<'asc' | 'desc'>('desc');

function sortValue(row: WorkspaceMemberRow, field: string): string | number {
  switch (field) {
    case 'role':
      return WORKSPACE_ROLE_RANK[row.role];
    case 'status':
      return row.status === 'active' ? 1 : 0;
    case 'joined':
      return new Date(row.accepted_at ?? row.invited_at).getTime();
    case 'name':
    default:
      return row.name;
  }
}

const sortedRows = computed(() => {
  const dir = sortDirection.value === 'asc' ? 1 : -1;
  return [...filteredRows.value].sort((a, b) => {
    const av = sortValue(a, sortField.value);
    const bv = sortValue(b, sortField.value);
    // Names collate through localeCompare, not raw `<`: code-unit
    // order pushes accented names ("Émile", "Öztürk") past "Zoe"
    // instead of interleaving them, which the fr/nl catalogues hit.
    if (av !== bv) {
      const cmp =
        typeof av === 'string' && typeof bv === 'string'
          ? av.localeCompare(bv)
          : av < bv
            ? -1
            : 1;
      return cmp * dir;
    }
    // Name is the stable tiebreak so equal-rank rows don't reorder
    // between renders.
    return a.name.localeCompare(b.name);
  });
});

function handleSortUpdate(field: string, direction: 'asc' | 'desc') {
  sortField.value = field;
  sortDirection.value = direction;
}

// Widths keep the identity column elastic and everything else at its
// natural size. Status and Joined drop out at the narrower desktop
// breakpoints, leaving identity + role, which is what the page is for.
const columns = computed(() => [
  {
    field: 'member',
    label: t('admin-workspace-members-col-user'),
    width: 'minmax(200px,1fr)',
    sortable: true,
    sortKey: 'name',
    responsive: 'always' as const,
  },
  {
    field: 'role',
    label: t('admin-workspace-members-col-role'),
    width: 'minmax(130px,170px)',
    sortable: true,
    responsive: 'always' as const,
  },
  {
    field: 'status',
    label: t('workspace-members-col-status'),
    width: 'minmax(100px,130px)',
    sortable: true,
    responsive: 'md' as const,
  },
  {
    field: 'joined',
    label: t('workspace-members-col-joined'),
    width: 'minmax(120px,160px)',
    sortable: true,
    responsive: 'lg' as const,
  },
  {
    field: 'actions',
    label: '',
    width: 'minmax(44px,60px)',
    sortable: false,
    responsive: 'always' as const,
  },
]);

const isFetching = computed(() => membersQuery.asyncStatus.value === 'loading');
const isFirstLoad = computed(() => isFetching.value && rows.value.length === 0);
const isBackgroundRefresh = computed(() => isFetching.value && rows.value.length > 0);

function isLastOwnerError(error: unknown): boolean {
  const e = error as { response?: { status?: number; data?: { error?: string } } };
  return e.response?.status === 409 && e.response?.data?.error === 'last_owner';
}
function isForbidden(error: unknown): boolean {
  return (error as { response?: { status?: number } }).response?.status === 403;
}
function resolveMutationError(error: unknown, fallbackKey: string): string {
  if (isLastOwnerError(error)) return t('admin-workspace-members-last-owner-hint');
  if (isForbidden(error)) return t('workspace-members-error-forbidden');
  return extractErrorMessage(error, t(fallbackKey));
}

/**
 * The PATCH returns the updated membership, so the success path patches
 * that one row into the cache instead of refetching the whole team. A
 * refetch flips `isBackgroundRefresh`, which DataTable renders as
 * `opacity-60 pointer-events-none` across every row — so promoting
 * several people in a row would grey the table out and swallow the next
 * click each time. Only the failure path refetches, to resync from
 * whatever the server actually holds.
 */
async function changeRole(member: WorkspaceMemberRow, role: WorkspaceRole) {
  if (role === member.role || !member.editable) return;
  isSaving.value = true;
  try {
    const updated = await workspacesService.updateWorkspaceMemberRole(member.user_uuid, { role });
    queryCache.setQueryData(
      MEMBERS_KEY,
      members.value.map((m) => (m.user_uuid === updated.user_uuid ? updated : m)),
    );
  } catch (e: unknown) {
    toast.error(resolveMutationError(e, 'admin-workspace-members-error-role'));
    refresh();
  } finally {
    isSaving.value = false;
  }
}

async function confirmRemove() {
  const member = memberToRemove.value;
  if (!member) return;
  isSaving.value = true;
  try {
    await workspacesService.removeWorkspaceMember(member.user_uuid);
    refresh();
  } catch (e: unknown) {
    toast.error(resolveMutationError(e, 'admin-workspace-members-error-remove'));
  } finally {
    isSaving.value = false;
    // Closed on failure too. The dialog and the toast container share a
    // z-index, and the dialog teleports in last, so a toast raised
    // behind an open dialog is invisible and auto-dismisses unseen.
    memberToRemove.value = null;
  }
}

function requestRemove(member: WorkspaceMemberRow) {
  if (member.editable) memberToRemove.value = member;
}
function openProfile(member: WorkspaceMemberRow) {
  void router.push(`/users/${member.user_uuid}`);
}

// In hosted, staff seats are managed in the control plane; this is the single
// "door" to Instances -> Seats that replaces the in-product staff controls.
function manageTeamInControlPlane() {
  void openControlPlaneSeats();
}
</script>

<template>
  <!-- Single root div, see `App.vue`'s Transition note. -->
  <div class="h-full">
    <!-- `error` is only raised when there's nothing to show. The layout
         folds it into `is-empty`, which swaps the whole body for an
         error card, so passing it unconditionally would let one failed
         background refetch wipe a loaded team list mid-audit. -->
    <ListPageLayout
      :items="sortedRows"
      :total-items="sortedRows.length"
      :is-first-load="isFirstLoad"
      :is-background-refresh="isBackgroundRefresh"
      :is-loading-more="false"
      :error="rows.length === 0 ? loadError || null : null"
      :search-query="searchQuery"
      :search-placeholder="$t('workspace-members-search-placeholder')"
      @update:search-query="searchQuery = $event"
      @retry="refresh"
    >
      <!-- Hosted: the roster is a read-only projection of control-plane-owned
           seats; this is the single hand-off to manage the team there. -->
      <template v-if="isHostedDeploymentRef" #view-tabs>
        <button
          type="button"
          @click="manageTeamInControlPlane"
          class="inline-flex items-center gap-2 rounded-lg border border-default bg-surface-alt px-3 py-1.5 text-sm font-medium text-secondary transition-colors hover:text-primary hover:border-strong"
        >
          <Icon name="openExternal" size="xs" />
          {{ $t('workspace-members-manage-in-control-plane') }}
        </button>
      </template>

      <template #empty-state>
        <EmptyState
          icon="users"
          :title="
            searchQuery
              ? $t('workspace-members-empty-search-title')
              : $t('empty-workspace-members-title')
          "
          :description="
            searchQuery
              ? $t('workspace-members-empty-search-description')
              : $t('workspace-members-empty-description')
          "
          :action-label="searchQuery ? undefined : $t('workspace-members-invite')"
          @action="navigateToInvite"
        />
      </template>

      <template #desktop="{ items, isBackgroundRefresh: refreshing }">
        <DataTable
          :columns="columns"
          :data="items"
          :selected-items="[]"
          :selectable="false"
          item-id-field="user_uuid"
          :sort-field="sortField"
          :sort-direction="sortDirection"
          :loading="refreshing"
          cell-padding="px-2 py-1.5"
          @update:sort="handleSortUpdate"
          @row-click="openProfile"
        >
          <template #cell-member="{ item }">
            <div class="flex min-w-0 items-center gap-2" :title="item.user_uuid">
              <UserInfoCell
                :user-id="item.user_uuid"
                :user-name="item.name"
                :email="item.email"
                :avatar="item.avatar"
                avatar-size="xs"
              />
              <span v-if="item.isYou" class="whitespace-nowrap text-xs text-tertiary">
                {{ $t('workspace-members-you') }}
              </span>
            </div>
          </template>

          <template #cell-role="{ item }">
            <MemberRoleMenu
              :member="item"
              :assignable="assignableRoles"
              :disabled="isSaving"
              @change="changeRole(item, $event)"
            />
          </template>

          <template #cell-status="{ item }">
            <StatusBadgeCell type="membership" :value="item.status" :label="item.statusLabel" />
          </template>

          <template #cell-joined="{ item }">
            <div class="flex min-w-0 items-center gap-1.5">
              <span v-if="item.status === 'pending'" class="text-xs text-tertiary">
                {{ $t('workspace-members-invited-label') }}
              </span>
              <DateCell :value="item.accepted_at ?? item.invited_at" format="clean-relative" />
            </div>
          </template>

          <!-- Hover-revealed only where a pointer can hover. The layout
               switches to the mobile body at 1024px, so tablets in
               landscape get this desktop table with no hover and no
               focus-visible from touch: gating on `group-hover` alone
               left them unable to remove anyone. `pointer-coarse`
               devices get a persistent button instead. -->
          <template #cell-actions="{ item }">
            <button
              v-if="item.editable"
              type="button"
              class="ml-auto rounded-md p-1.5 text-secondary transition-colors hover:bg-status-error/10 hover:text-status-error focus-visible:opacity-100 pointer-fine:opacity-0 pointer-fine:group-hover:opacity-100"
              :aria-label="$t('admin-workspace-members-remove')"
              :title="$t('admin-workspace-members-remove')"
              :disabled="isSaving"
              @click.stop="requestRemove(item)"
            >
              <Icon name="trash" size="xs" />
            </button>
          </template>
        </DataTable>
      </template>

      <template #mobile-row="{ item }">
        <div
          class="flex items-center gap-3 border-t border-default px-3 py-2 transition-colors first:border-t-0 hover:bg-surface-hover active:bg-surface-alt"
          @click="openProfile(item)"
        >
          <UserAvatar
            :uuid="item.user_uuid"
            :fallbackName="item.name"
            :fallbackAvatar="item.avatar"
            size="sm"
            :clickable="false"
            :show-name="false"
            class="flex-shrink-0"
          />
          <!-- The name gets the full width; role, email and the
               pending chip share a wrapping meta line below it. Four
               competing columns left roughly 160px for the name on a
               375px phone, which truncated most real names. -->
          <div class="flex min-w-0 flex-1 flex-col gap-0.5">
            <div class="truncate text-sm font-medium text-primary">
              {{ item.name }}
              <span v-if="item.isYou" class="font-normal text-tertiary">
                {{ $t('workspace-members-you') }}
              </span>
            </div>
            <!-- Only the exceptional state is badged. An accepted
                 member is the norm and doesn't need a chip. -->
            <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
              <MemberRoleMenu
                :member="item"
                :assignable="assignableRoles"
                :disabled="isSaving"
                @change="changeRole(item, $event)"
              />
              <span v-if="item.email" class="truncate text-tertiary">{{ item.email }}</span>
              <StatusBadgeCell
                v-if="item.status === 'pending'"
                type="membership"
                :value="item.status"
                :label="item.statusLabel"
              />
            </div>
          </div>
          <!-- Touch has no hover, so the destructive action is
               persistent here and sized to a 44px tap target. -->
          <button
            v-if="item.editable"
            type="button"
            class="flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-md text-secondary active:bg-status-error/10 active:text-status-error"
            :aria-label="$t('admin-workspace-members-remove')"
            :disabled="isSaving"
            @click.stop="requestRemove(item)"
          >
            <Icon name="trash" size="sm" />
          </button>
        </div>
      </template>
    </ListPageLayout>

    <ConfirmModal
      :show="memberToRemove !== null"
      variant="danger"
      :title="$t('admin-workspace-members-remove-confirm-title')"
      :message="
        memberToRemove
          ? $t('admin-workspace-members-remove-confirm-message', { name: memberToRemove.name })
          : ''
      "
      :confirm-label="$t('admin-workspace-members-remove-confirm-label')"
      :loading="isSaving"
      @confirm="confirmRemove"
      @close="memberToRemove = null"
    />
  </div>
</template>
