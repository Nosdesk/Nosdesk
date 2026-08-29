<script setup lang="ts">
import { computed, ref, useTemplateRef } from 'vue'
import { useRouter } from 'vue-router'
import { useMutation, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { extractErrorMessage } from '@/utils/errors'
import { formatDate } from '@nosdesk/core/utils/dateUtils'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { isHostedDeployment, getControlPlaneUrl } from '@nosdesk/core/services/instanceConfig'

import DataTable from '@/components/common/DataTable.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'
import BulkConfirmDialog from '@/components/common/BulkConfirmDialog.vue'
import ListPageLayout, { type ListPageLayoutExpose } from '@/components/common/ListPageLayout.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Modal from '@/components/Modal.vue'
import ListViewToolbar from '@/components/views/ListViewToolbar.vue'
import ListViewModals from '@/components/views/ListViewModals.vue'
import { useListView } from '@/composables/useListView'
import type { ChipFacetDef } from '@/composables/useChipFiltersFromControls'
import type { GroupAxisDef } from '@/composables/useListGrouping'
import { useAuthStore } from '@/stores/auth'

import { StatusBadgeCell, UserInfoCell, DateCell } from '@/components/common/cells'
import UserAvatar from '@/components/UserAvatar.vue'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import userService from '@/services/userService'
import { usersKeys } from '@nosdesk/core/queries/users'
import { effectiveRole, type User, type UserRole } from '@nosdesk/core/types/user'

defineOptions({ name: 'UsersListView' })

const router = useRouter()
const queryCache = useQueryCache()
const { isMobile } = useMobileDetection()
const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const toast = useToastStore()
const auth = useAuthStore()
const userUuid = computed<string | null>(() => auth.user?.uuid ?? null)

const layoutRef = useTemplateRef<ListPageLayoutExpose>('layout')
const scrollContainerRef = computed<HTMLElement | null>(
  () => layoutRef.value?.scrollContainerRef ?? null,
)

const navigateToCreateUser = () => {
  // In hosted mode identity is owned by the control plane; a user created here
  // can't sign in. Hand off to the control-plane dashboard (Instances -> Seats)
  // rather than open a create form that produces a dead account.
  if (isHostedDeployment()) {
    const cp = getControlPlaneUrl()
    if (cp) window.open(`${cp}/instances`, '_blank', 'noopener')
    toast.info(t('user-mgmt-hosted-add-in-control-plane'))
    return
  }
  void router.push('/users/new')
}
const navigateToUser = (user: User) => {
  void router.push(`/users/${user.uuid}`)
}
usePageCreateAction(navigateToCreateUser)

// Filter facets. Role is multi-select (backend accepts CSV via
// parse_role); Name is the chip text-facet that drives
// controls.searchQuery. Active vs soft-deleted is the `deleted`
// filter, but it's driven by the Active/Deleted view tab (see the
// #view-tabs slot) rather than a buried chip, since admins need to
// find the deleted view to restore an accidentally deleted user.
const userFacets = computed<ChipFacetDef[]>(() => [
  {
    key: 'name',
    labelKey: 'user-mgmt-filter-name-label',
    kind: 'text',
    searchInput: true,
    options: () => [],
  },
  {
    key: 'role',
    labelKey: 'user-mgmt-filter-role-label',
    kind: 'multi',
    options: () => [
      { value: 'admin', label: t('user-mgmt-role-admin'), swatchClass: 'bg-rose-500' },
      { value: 'technician', label: t('user-mgmt-role-technician'), swatchClass: 'bg-accent' },
      { value: 'audit_reviewer', label: t('user-mgmt-role-audit_reviewer'), swatchClass: 'bg-purple-500' },
      { value: 'user', label: t('user-mgmt-role-user'), swatchClass: 'bg-zinc-400' },
    ],
  },
])

// Group-by axes. Role uses severity order; status splits active
// vs soft-deleted; joined buckets recent vs older to spot recent
// hires during onboarding.
const ROLE_ORDER: Array<UserRole> = ['admin', 'technician', 'audit_reviewer', 'user']
const JOIN_BUCKET_ORDER = ['this-month', 'this-year', 'older'] as const

function joinBucket(createdAt: string): (typeof JOIN_BUCKET_ORDER)[number] {
  const created = new Date(createdAt)
  const now = new Date()
  const thirtyDaysAgo = new Date(now)
  thirtyDaysAgo.setDate(thirtyDaysAgo.getDate() - 30)
  if (created >= thirtyDaysAgo) return 'this-month'
  if (created.getFullYear() === now.getFullYear()) return 'this-year'
  return 'older'
}

const groupAxes: GroupAxisDef<User>[] = [
  {
    key: 'role',
    labelKey: 'user-mgmt-grouping-role',
    bucketFor: (user) => ({
      key: `role:${effectiveRole(user)}`,
      label: t(`user-mgmt-role-${effectiveRole(user)}`),
    }),
    sortBy: (bucketKey) => {
      const v = bucketKey.replace('role:', '') as UserRole
      const idx = ROLE_ORDER.indexOf(v)
      return idx === -1 ? 999 : idx
    },
  },
  {
    key: 'status',
    labelKey: 'user-mgmt-grouping-status',
    bucketFor: (user) => {
      const isDeleted = user.deleted_at != null
      return {
        key: `status:${isDeleted ? 'deleted' : 'active'}`,
        label: isDeleted
          ? t('user-mgmt-grouping-status-deleted')
          : t('user-mgmt-grouping-status-active'),
      }
    },
    sortBy: (bucketKey) => (bucketKey === 'status:active' ? 0 : 1),
  },
  {
    key: 'joined',
    labelKey: 'user-mgmt-grouping-joined',
    bucketFor: (user) => {
      const b = joinBucket(user.created_at)
      return {
        key: `joined:${b}`,
        label: t(`user-mgmt-grouping-joined-${b}`),
      }
    },
    sortBy: (bucketKey) => {
      const v = bucketKey.replace('joined:', '') as (typeof JOIN_BUCKET_ORDER)[number]
      const idx = JOIN_BUCKET_ORDER.indexOf(v)
      return idx === -1 ? 999 : idx
    },
  },
]

/**
 * Every column is server-sortable: `name`, `role`, `email`,
 * `created_at`, `open_ticket_count` and `device_count` all have match
 * arms in `backend/src/repository/users.rs`. The last three sort via
 * correlated subqueries, since email and the two counts aren't `users`
 * columns — the handler enriches each page with them after LIMIT.
 *
 * Widths use bounded px maxes rather than `auto` so the `1fr` identity
 * column keeps the slack: grid grows non-flexible tracks to their
 * growth limit before expanding `fr` ones, so an `auto` max lets a long
 * email starve the name column down to min-content. Mins are sized to
 * stay under the ~768px the desktop table gets at its narrowest (a
 * 1024px viewport less the 256px navbar); the grid is clipped, not
 * scrollable, if they don't.
 */
const columns = computed(() => [
  { field: 'user', label: t('user-mgmt-column-user'), width: 'minmax(160px,1fr)', sortable: true, sortKey: 'name', responsive: 'always' as const },
  { field: 'email', label: t('user-mgmt-column-email'), width: 'minmax(150px,260px)', sortable: true, responsive: 'always' as const },
  { field: 'role', label: t('user-mgmt-column-role'), width: 'minmax(90px,120px)', sortable: true, responsive: 'always' as const },
  { field: 'open_ticket_count', label: t('user-mgmt-column-tickets'), width: 'minmax(70px,90px)', sortable: true, responsive: 'md' as const },
  { field: 'device_count', label: t('user-mgmt-column-assets'), width: 'minmax(70px,90px)', sortable: true, responsive: 'md' as const },
  { field: 'created_at', label: t('user-mgmt-column-joined'), width: 'minmax(110px,150px)', sortable: true, responsive: 'lg' as const, defaultHidden: true },
])

// Shell composable bundling controls + page + selection + chip
// filters + grouping + columns + saved-view round-trip. The
// `user` column is pinned (primary identifier stays anchored)
// and `created_at` is default-hidden so the first paint stays
// focused on identity + activity.
const listView = useListView({
  dataset: 'users',
  userUuid,
  t,
  itemIdField: 'uuid',
  itemId: (u: User) => u.uuid,
  defaultSortField: 'name',
  pageKeys: usersKeys,
  // The "deleted" chip lives in controls.filters; the fetcher
  // reads it back to swap the backend WHERE clause.
  fetchPage: (params) =>
    userService.getPaginatedUsers({
      ...params,
      deleted:
        typeof params.deleted === 'string' && params.deleted === 'deleted'
          ? 'deleted'
          : 'active',
    }),
  // Any user change (create / update / soft-delete / restore / purge)
  // arrives as a `user`-aggregate sync action and invalidates both the
  // active and deleted lists without manual cache busts.
  syncAggregates: ['user'],
  mobileSearch: {
    placeholder: t('user-mgmt-search-placeholder'),
    createIcon: 'user',
    onCreate: navigateToCreateUser,
  },
  urlSyncParamKeys: ['role', 'deleted'],
  scrollContainerRef,
  facets: userFacets,
  groupAxes,
  columns,
  pinnedColumnIds: ['user'],
})

// The deleted-users view is a platform-admin-only recovery surface
// (matching who can restore/purge). The backend independently forces the
// active filter for non-admins, so hiding the tab is the UI half of that
// gate, not the enforcement.
const isPlatformAdmin = computed(() => auth.user?.platform_role === 'platform_admin')

// Active vs Deleted view. The "deleted" filter lives in the list-view
// controls; the view tab toggles it so soft-deleted users (with their
// per-row Restore / Purge actions) are one obvious click away instead of
// hidden behind a filter chip.
const isDeletedView = computed(() => {
  const v = listView.controls.filters.value['deleted']
  return Array.isArray(v) ? v.includes('deleted') : v === 'deleted'
})
function setDeletedView(deleted: boolean) {
  if (deleted === isDeletedView.value) return
  listView.chipFilters.toggleValue('deleted', 'deleted')
}

// Bulk delete: irreversible, so a confirm modal rather than the
// optimistic Undo-toast pattern. Bulk role change: a domain-
// specific picker (Modal with role buttons), no confirm step.
const showDeleteConfirm = ref(false)
const showRoleModal = ref(false)

const ROLE_OPTIONS = computed(() => [
  { value: 'admin', label: t('user-mgmt-role-admin') },
  { value: 'technician', label: t('user-mgmt-role-technician') },
  { value: 'audit_reviewer', label: t('user-mgmt-role-audit_reviewer') },
  { value: 'user', label: t('user-mgmt-role-user') },
])

const bulkActionMutation = useMutation({
  mutation: (vars: { action: 'delete' | 'set-role'; ids: string[]; value?: string }) =>
    userService.bulkAction(vars),
  onSettled: () => queryCache.invalidateQueries({ key: usersKeys.root }),
  onError: (err) => {
    console.error('Bulk action failed:', err)
    toast.error(extractErrorMessage(err, t('user-mgmt-bulk-action-error')))
  },
})

async function confirmDelete() {
  showDeleteConfirm.value = false
  const ids = listView.selection.selectedIds.value
  if (ids.length === 0) return
  await bulkActionMutation.mutateAsync({ action: 'delete', ids })
  listView.selection.clear()
}

async function applyRoleChange(role: string) {
  const ids = listView.selection.selectedIds.value
  if (ids.length === 0) return
  await bulkActionMutation.mutateAsync({ action: 'set-role', ids, value: role })
  listView.selection.clear()
  showRoleModal.value = false
}

// Per-row controls in the "Show deleted" view. Restore is one
// click; permanent delete needs a confirm modal since it skips
// the retention worker and there's no undo. Both invalidate the
// user list cache so the row re-renders or disappears immediately.
const purgeTarget = ref<User | null>(null)

async function restoreUser(user: User) {
  const ok = await userService.restoreUser(user.uuid)
  if (ok) {
    toast.success(t('user-mgmt-restored', { name: user.name }))
    void queryCache.invalidateQueries({ key: usersKeys.root })
  } else {
    toast.error(t('user-mgmt-restore-error'))
  }
}

async function confirmPurge() {
  const target = purgeTarget.value
  purgeTarget.value = null
  if (!target) return
  const ok = await userService.purgeUserNow(target.uuid)
  if (ok) {
    toast.success(t('user-mgmt-purged', { name: target.name }))
    void queryCache.invalidateQueries({ key: usersKeys.root })
  } else {
    toast.error(t('user-mgmt-purge-error'))
  }
}

function formatPurgeAt(deletedAt: string): string {
  const dt = new Date(deletedAt)
  // 30-day grace matches NOSDESK_USER_PURGE_GRACE_DAYS default;
  // close enough for the chip — the real countdown lives on the
  // detail view.
  dt.setDate(dt.getDate() + 30)
  return formatDate(dt)
}
</script>

<template>
  <!-- Single root div, see `App.vue`'s Transition note. -->
  <div class="h-full">
    <ListPageLayout
      ref="layout"
      :items="listView.page.items.value"
      :total-items="listView.page.totalItems.value"
      :is-first-load="listView.page.isFirstLoad.value"
      :is-background-refresh="listView.page.isBackgroundRefresh.value"
      :is-loading-more="listView.page.isLoadingMore.value"
      :error="listView.page.errorMessage.value"
      :search-query="listView.controls.searchQuery.value"
      :search-placeholder="$t('user-mgmt-search-placeholder')"
      :item-label="$t('user-mgmt-item-label')"
      bulk-selection-copy-key="bulk-bar-users-selected"
      bulk-all-selected-copy-key="bulk-bar-users-all-selected"
      :bulk-selection="listView.selection"
      hide-desktop-search
      @update:search-query="listView.controls.handleSearchUpdate"
      @retry="listView.page.handleRetry"
    >
      <template #view-tabs>
        <div
          v-if="isPlatformAdmin"
          class="inline-flex items-center gap-0.5 rounded-lg border border-default bg-surface-alt p-0.5"
        >
          <button
            type="button"
            class="px-3 py-1 text-sm font-medium rounded-md transition-colors"
            :class="!isDeletedView ? 'bg-surface text-primary shadow-sm' : 'text-secondary hover:text-primary'"
            @click="setDeletedView(false)"
          >
            {{ $t('user-mgmt-tab-active') }}
          </button>
          <button
            type="button"
            class="px-3 py-1 text-sm font-medium rounded-md transition-colors"
            :class="isDeletedView ? 'bg-surface text-primary shadow-sm' : 'text-secondary hover:text-primary'"
            @click="setDeletedView(true)"
          >
            {{ $t('user-mgmt-tab-deleted') }}
          </button>
        </div>
      </template>

      <template #filters>
        <ListViewToolbar
          :list-view="listView"
          :switcher-placeholder="$t('views-user-switcher-placeholder')"
          @open-editor="listView.openEditor"
          @save-as="listView.showSaveModal.value = true"
        />
      </template>

      <template #empty-state>
        <EmptyState
          v-if="isDeletedView"
          icon="trash"
          :title="$t('empty-users-deleted-title')"
          :description="$t('empty-users-deleted-description')"
        />
        <EmptyState
          v-else
          icon="users"
          :title="listView.controls.searchQuery.value ? $t('empty-users-search-title') : $t('empty-users-default-title')"
          :description="listView.controls.searchQuery.value ? $t('empty-users-search-description') : $t('empty-users-default-description')"
          :action-label="!listView.controls.searchQuery.value ? $t('user-mgmt-invite-action') : undefined"
          @action="navigateToCreateUser"
        />
      </template>

      <template #desktop="{ items, isBackgroundRefresh }">
        <DataTable
          :columns="listView.tableColumns.visible.value"
          :data="items"
          :buckets="listView.buckets.value"
          :is-collapsed="listView.grouping.isCollapsed"
          :selected-items="listView.dt.selectedItems"
          item-id-field="uuid"
          :sort-field="listView.controls.sortField.value"
          :sort-direction="listView.controls.sortDirection.value"
          :column-reorder="listView.tableColumns.reorderBundle"
          :column-resize="listView.tableColumns.resizeBundle"
          :loading="isBackgroundRefresh"
          @update:sort="listView.controls.handleSortUpdate"
          @toggle-selection="listView.dt.onToggleSelection"
          @toggle-all="listView.dt.onToggleAll"
          @row-click="navigateToUser"
          @toggle-bucket="listView.grouping.toggleCollapsed"
        >
          <template #cell-user="{ item }">
            <div class="flex items-center gap-2 min-w-0">
              <!-- Name only. Email has its own sortable column now, so
                   stacking it here would print it twice and force a
                   two-line row. -->
              <UserInfoCell
                :user-id="item.uuid"
                :user-name="item.name"
                :avatar="item.avatar_thumb || item.avatar_url"
                :show-avatar="true"
              />
              <span
                v-if="item.deleted_at"
                class="inline-flex items-center rounded bg-status-error/10 px-1.5 py-0.5 text-xs font-medium text-status-error whitespace-nowrap"
                :title="$t('user-mgmt-deleted-purges-on', { date: formatPurgeAt(item.deleted_at) })"
              >
                {{ $t('user-mgmt-deleted-badge') }}
              </span>
              <template v-if="item.deleted_at">
                <button
                  type="button"
                  class="ml-1 rounded p-1 text-secondary hover:bg-surface-hover hover:text-primary"
                  :title="$t('user-mgmt-restore')"
                  @click.stop="restoreUser(item)"
                >
                  <svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 10h10a8 8 0 010 16v-3M3 10l4-4M3 10l4 4" />
                  </svg>
                </button>
                <button
                  type="button"
                  class="rounded p-1 text-secondary hover:bg-status-error/10 hover:text-status-error"
                  :title="$t('user-mgmt-purge-now')"
                  @click.stop="purgeTarget = item"
                >
                  <svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                </button>
              </template>
            </div>
          </template>

          <!-- The primary address from `user_emails`; a user can hold
               several, and rows without one sort last rather than
               leading an A-Z sort with a blank cell. -->
          <template #cell-email="{ item }">
            <span v-if="item.email" class="text-sm text-secondary truncate">{{ item.email }}</span>
            <span v-else class="text-sm text-tertiary">{{ $t('user-mgmt-no-email') }}</span>
          </template>

          <template #cell-role="{ item }">
            <StatusBadgeCell type="role" :value="effectiveRole(item)" />
          </template>

          <template #cell-open_ticket_count="{ item }">
            <span class="text-sm text-secondary tabular-nums">{{ item.open_ticket_count ?? 0 }}</span>
          </template>

          <template #cell-device_count="{ item }">
            <span class="text-sm text-secondary tabular-nums">{{ item.device_count ?? 0 }}</span>
          </template>

          <template #cell-created_at="{ item }">
            <DateCell :value="item.created_at" format="clean-relative" />
          </template>
        </DataTable>
      </template>

      <template #mobile-row="{ item }">
        <div
          class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer border-t border-default first:border-t-0"
          @click="navigateToUser(item)"
        >
          <UserAvatar
            :uuid="item.uuid"
            :fallbackName="item.name"
            :fallbackAvatar="item.avatar_thumb || item.avatar_url"
            size="sm"
            :clickable="false"
            :show-name="false"
            class="flex-shrink-0"
          />
          <div class="flex-1 min-w-0">
            <div class="text-sm text-primary font-medium truncate">{{ item.name }}</div>
            <div class="flex flex-wrap items-center gap-2 mt-1 text-xs">
              <span v-if="item.email" class="text-tertiary truncate max-w-[200px]">{{ item.email }}</span>
              <span
                class="inline-flex items-center px-1.5 py-0.5 rounded font-medium capitalize"
                :class="{
                  'bg-status-error-muted text-status-error': effectiveRole(item) === 'admin',
                  'bg-accent-muted text-accent': effectiveRole(item) === 'technician',
                  'bg-purple-500/10 text-purple-700 dark:text-purple-400': effectiveRole(item) === 'audit_reviewer',
                  'bg-surface-alt text-secondary': effectiveRole(item) === 'user',
                }"
              >
                {{ effectiveRole(item).replace('_', ' ') }}
              </span>
              <span v-if="item.open_ticket_count" class="text-secondary tabular-nums">
                {{ $t('user-mgmt-mobile-tickets', { count: item.open_ticket_count }) }}
              </span>
              <span v-if="item.device_count" class="text-secondary tabular-nums">
                {{ $t('user-mgmt-mobile-assets', { count: item.device_count }) }}
              </span>
            </div>
          </div>
          <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
          </svg>
        </div>
      </template>

      <template #bulk-actions="{ selectedCount }">
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-secondary hover:text-primary hover:bg-surface-hover transition-colors whitespace-nowrap disabled:opacity-50"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          @click="showRoleModal = true"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
          </svg>
          {{ $t('user-mgmt-bulk-role') }}
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-status-error hover:bg-status-error/10 transition-colors whitespace-nowrap disabled:opacity-50"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          @click="showDeleteConfirm = true"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
          </svg>
          {{ selectedCount > 0 ? $t('user-mgmt-bulk-delete-count', { count: selectedCount }) : $t('user-mgmt-bulk-delete') }}
        </button>
      </template>

      <template #footer>
        <PaginationControls
          v-if="!isMobile"
          :current-page="listView.controls.currentPage.value"
          :total-pages="listView.page.totalPages.value"
          :total-items="listView.page.totalItems.value"
          :page-size="listView.controls.pageSize.value"
          :page-size-options="listView.controls.pageSizeOptions"
          :is-infinite-mode="listView.controls.isInfiniteMode.value"
          @update:current-page="listView.controls.handlePageChange"
          @update:page-size="listView.controls.handlePageSizeChange"
        />
      </template>
    </ListPageLayout>

    <BulkConfirmDialog
      :show="showDeleteConfirm"
      :title="$t('user-mgmt-bulk-delete-title', { count: listView.selection.selectedCount.value })"
      :message="$t('user-mgmt-bulk-delete-message', { count: listView.selection.selectedCount.value })"
      :confirm-label="$t('user-mgmt-bulk-delete-count', { count: listView.selection.selectedCount.value })"
      @confirm="confirmDelete"
      @close="showDeleteConfirm = false"
    />

    <BulkConfirmDialog
      :show="purgeTarget !== null"
      :title="$t('user-mgmt-purge-title')"
      :message="$t('user-mgmt-purge-message', { name: purgeTarget?.name ?? '' })"
      :confirm-label="$t('user-mgmt-purge-confirm')"
      @confirm="confirmPurge"
      @close="purgeTarget = null"
    />

    <Modal
      :show="showRoleModal"
      :title="$t('user-mgmt-role-modal-title')"
      size="sm"
      @close="showRoleModal = false"
    >
      <div class="flex flex-col gap-2 p-4">
        <p class="text-sm text-secondary mb-2">
          {{ $t('user-mgmt-role-modal-body', { count: listView.selection.selectedCount.value }) }}
        </p>
        <button
          v-for="role in ROLE_OPTIONS"
          :key="role.value"
          :disabled="bulkActionMutation.asyncStatus.value === 'loading'"
          class="flex items-center gap-3 px-4 py-3 rounded-lg hover:bg-surface-hover transition-colors text-left disabled:opacity-50"
          @click="applyRoleChange(role.value)"
        >
          <StatusBadgeCell type="role" :value="role.value" />
          <span class="text-primary">{{ role.label }}</span>
        </button>
      </div>
    </Modal>

    <ListViewModals :list-view="listView" />
  </div>
</template>
