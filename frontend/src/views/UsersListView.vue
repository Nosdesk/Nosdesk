<script setup lang="ts">
import { computed, ref, useTemplateRef } from 'vue'
import { useRouter } from 'vue-router'
import { useMutation, useQueryCache } from '@pinia/colada'

import DataTable from '@/components/common/DataTable.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'
import BulkConfirmDialog from '@/components/common/BulkConfirmDialog.vue'
import ListPageLayout, { type ListPageLayoutExpose } from '@/components/common/ListPageLayout.vue'
import FilterRow from '@/components/common/FilterRow.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Modal from '@/components/Modal.vue'

import { StatusBadgeCell, UserInfoCell, DateCell } from '@/components/common/cells'
import UserAvatar from '@/components/UserAvatar.vue'
import { useListControls } from '@/composables/useListControls'
import { useListPage } from '@/composables/useListPage'
import { useBulkSelection } from '@/composables/useBulkSelection'
import { useBulkSelectionForDataTable } from '@/composables/useBulkSelectionForDataTable'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { useDataStore } from '@/stores/dataStore'
import userService from '@/services/userService'
import { usersKeys } from '@/queries/users'
import type { User } from '@/types/user'

defineOptions({ name: 'UsersListView' })

const router = useRouter()
const dataStore = useDataStore()
const queryCache = useQueryCache()
const { isMobile } = useMobileDetection()

const layoutRef = useTemplateRef<ListPageLayoutExpose>('layout')
const scrollContainerRef = computed<HTMLElement | null>(
  () => layoutRef.value?.scrollContainerRef ?? null,
)

const navigateToCreateUser = () => {
  void router.push('/users/new')
}
const navigateToUser = (user: User) => {
  void router.push(`/users/${user.uuid}`)
}

const controls = useListControls<User>({
  itemIdField: 'uuid',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  defaultPageSize: 0,
})

// Goes through `dataStore.getPaginatedUsers` rather than the raw
// userService so the dashboard's user-name lookups (used by
// avatar rendering across other widgets) continue to share the
// same in-memory cache. Pinia Colada wraps that for us.
const page = useListPage({
  controls,
  keys: usersKeys,
  fetchPage: (params) => dataStore.getPaginatedUsers(params),
  scrollContainerRef,
  sseEvents: ['user-updated', 'user-created', 'user-deleted'],
  mobileSearch: {
    placeholder: 'Search users...',
    createIcon: 'user',
    onCreate: navigateToCreateUser,
  },
  urlSync: { paramKeys: ['role'] },
})

usePageCreateAction(navigateToCreateUser)

const selection = useBulkSelection<User>({
  items: page.items,
  cacheKey: controls.cacheKeyPart,
  totalCount: page.totalItems,
  itemId: (u) => u.uuid,
})
const dt = useBulkSelectionForDataTable(selection)

const filterOptions = computed(() =>
  controls.buildFilterOptions({
    role: {
      options: [
        { value: 'admin', label: 'Admin' },
        { value: 'technician', label: 'Technician' },
        { value: 'user', label: 'User' },
      ],
      width: 'w-[140px]',
      allLabel: 'All Roles',
    },
  }),
)

const columns = [
  { field: 'user', label: 'User', width: '1fr', sortable: true, sortKey: 'name', responsive: 'always' as const },
  { field: 'role', label: 'Role', width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const },
  { field: 'open_ticket_count', label: 'Tickets', width: 'minmax(80px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'device_count', label: 'Devices', width: 'minmax(80px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'created_at', label: 'Joined', width: 'minmax(140px,auto)', sortable: false, responsive: 'lg' as const },
]

const gridClass =
  'grid-cols-[auto_1fr_minmax(100px,auto)] md:grid-cols-[auto_1fr_minmax(100px,auto)_minmax(80px,auto)_minmax(80px,auto)] lg:grid-cols-[auto_1fr_minmax(100px,auto)_minmax(80px,auto)_minmax(80px,auto)_minmax(140px,auto)]'

// Bulk delete: irreversible, so a confirm modal rather than the
// optimistic Undo-toast pattern. Bulk role change: a domain-
// specific picker (Modal with role buttons), no confirm step.
const showDeleteConfirm = ref(false)
const showRoleModal = ref(false)

const ROLE_OPTIONS = [
  { value: 'admin', label: 'Admin' },
  { value: 'technician', label: 'Technician' },
  { value: 'user', label: 'User' },
]

const bulkActionMutation = useMutation({
  mutation: (vars: { action: 'delete' | 'set-role'; ids: string[]; value?: string }) =>
    userService.bulkAction(vars),
  onSettled: () => queryCache.invalidateQueries({ key: usersKeys.root }),
  onError: (err) => {
    console.error('Bulk action failed:', err)
    alert('Failed to perform bulk action. Please try again.')
  },
})

async function confirmDelete() {
  showDeleteConfirm.value = false
  const ids = selection.selectedIds.value
  if (ids.length === 0) return
  await bulkActionMutation.mutateAsync({ action: 'delete', ids })
  selection.clear()
}

async function applyRoleChange(role: string) {
  const ids = selection.selectedIds.value
  if (ids.length === 0) return
  await bulkActionMutation.mutateAsync({ action: 'set-role', ids, value: role })
  selection.clear()
  showRoleModal.value = false
}
</script>

<template>
  <!-- Single root div, see `App.vue`'s Transition note. -->
  <div class="h-full">
    <ListPageLayout
      ref="layout"
      :items="page.items.value"
      :total-items="page.totalItems.value"
      :is-first-load="page.isFirstLoad.value"
      :is-background-refresh="page.isBackgroundRefresh.value"
      :is-loading-more="page.isLoadingMore.value"
      :error="page.errorMessage.value"
      :search-query="controls.searchQuery.value"
      search-placeholder="Search users..."
      item-label="user"
      :bulk-selection="selection"
      @update:search-query="controls.handleSearchUpdate"
      @retry="page.handleRetry"
    >
      <template #filters>
        <FilterRow
          :options="filterOptions"
          @update="controls.handleFilterUpdate"
          @reset="controls.resetFilters"
        />
      </template>

      <template #empty-state>
        <EmptyState
          icon="users"
          :title="controls.searchQuery.value ? 'No users match your search' : 'No users found'"
          :description="controls.searchQuery.value ? 'Try adjusting your search criteria' : 'Invite users to get started'"
          :action-label="!controls.searchQuery.value ? 'Invite User' : undefined"
          @action="navigateToCreateUser"
        />
      </template>

      <template #desktop="{ items, isBackgroundRefresh }">
        <DataTable
          :columns="columns"
          :data="items"
          :selected-items="dt.selectedItems"
          item-id-field="uuid"
          :sort-field="controls.sortField.value"
          :sort-direction="controls.sortDirection.value"
          :grid-class="gridClass"
          :loading="isBackgroundRefresh"
          @update:sort="controls.handleSortUpdate"
          @toggle-selection="dt.onToggleSelection"
          @toggle-all="dt.onToggleAll"
          @row-click="navigateToUser"
        >
          <template #cell-user="{ item }">
            <UserInfoCell
              :user-id="item.uuid"
              :user-name="item.name"
              :email="item.email"
              :avatar="item.avatar_thumb || item.avatar_url"
              :show-avatar="true"
            />
          </template>

          <template #cell-role="{ value }">
            <StatusBadgeCell type="role" :value="value" />
          </template>

          <template #cell-open_ticket_count="{ value }">
            <span class="text-sm text-secondary tabular-nums">{{ value ?? 0 }}</span>
          </template>

          <template #cell-device_count="{ value }">
            <span class="text-sm text-secondary tabular-nums">{{ value ?? 0 }}</span>
          </template>

          <template #cell-created_at="{ value }">
            <DateCell :value="value" format="clean-relative" />
          </template>
        </DataTable>
      </template>

      <template #mobile-row="{ item }">
        <div
          class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer border-t border-default first:border-t-0"
          @click="navigateToUser(item)"
        >
          <UserAvatar
            :name="item.uuid"
            :user-name="item.name"
            :avatar="item.avatar_thumb || item.avatar_url"
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
                  'bg-status-error-muted text-status-error': item.role === 'admin',
                  'bg-accent-muted text-accent': item.role === 'technician',
                  'bg-surface-alt text-secondary': item.role === 'user',
                }"
              >
                {{ item.role }}
              </span>
              <span v-if="item.open_ticket_count" class="text-secondary tabular-nums">
                {{ item.open_ticket_count }} ticket{{ item.open_ticket_count !== 1 ? 's' : '' }}
              </span>
              <span v-if="item.device_count" class="text-secondary tabular-nums">
                {{ item.device_count }} device{{ item.device_count !== 1 ? 's' : '' }}
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
          Role
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
          Delete{{ selectedCount > 0 ? ` ${selectedCount}` : '' }}
        </button>
      </template>

      <template #footer>
        <PaginationControls
          v-if="!isMobile"
          :current-page="controls.currentPage.value"
          :total-pages="page.totalPages.value"
          :page-size="controls.pageSize.value"
          :page-size-options="controls.pageSizeOptions"
          :show-import="true"
          @update:current-page="controls.handlePageChange"
          @update:page-size="controls.handlePageSizeChange"
          @import="() => {}"
        />
      </template>
    </ListPageLayout>

    <BulkConfirmDialog
      :show="showDeleteConfirm"
      :count="selection.selectedCount.value"
      item-label="user"
      action-verb="delete"
      @confirm="confirmDelete"
      @close="showDeleteConfirm = false"
    />

    <Modal
      :show="showRoleModal"
      title="Set Role"
      size="sm"
      @close="showRoleModal = false"
    >
      <div class="flex flex-col gap-2 p-4">
        <p class="text-sm text-secondary mb-2">
          Update role for {{ selection.selectedCount.value }} user{{ selection.selectedCount.value !== 1 ? 's' : '' }}
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
  </div>
</template>
