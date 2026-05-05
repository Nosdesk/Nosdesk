<script setup lang="ts">
import { computed, ref, useTemplateRef } from 'vue'
import { useRouter } from 'vue-router'
import { useMutation, useQueryCache } from '@pinia/colada'
import { extractErrorMessage } from '@/utils/errors'

import DataTable from '@/components/common/DataTable.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'
import BulkConfirmDialog from '@/components/common/BulkConfirmDialog.vue'
import ListPageLayout, { type ListPageLayoutExpose } from '@/components/common/ListPageLayout.vue'
import FilterRow from '@/components/common/FilterRow.vue'
import EmptyState from '@/components/common/EmptyState.vue'

import { TextCell, StatusBadgeCell, UserAvatarCell } from '@/components/common/cells'
import { useListControls } from '@/composables/useListControls'
import { useListPage } from '@/composables/useListPage'
import { useBulkSelection } from '@/composables/useBulkSelection'
import { useBulkSelectionForDataTable } from '@/composables/useBulkSelectionForDataTable'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { useDataStore } from '@/stores/dataStore'
import { getPaginatedDevices, bulkAction } from '@/services/deviceService'
import { devicesKeys } from '@/queries/devices'
import type { Device } from '@/types/device'

defineOptions({ name: 'DevicesListView' })

const router = useRouter()
const dataStore = useDataStore()
const queryCache = useQueryCache()
const { isMobile } = useMobileDetection()

// `useTemplateRef` (Vue 3.5+) typed against the layout's exported
// expose interface. TypeScript can't `InstanceType<>` a generic
// component, so the layout exports `ListPageLayoutExpose` as the
// public typed shape of its `defineExpose` payload.
const layoutRef = useTemplateRef<ListPageLayoutExpose>('layout')
const scrollContainerRef = computed<HTMLElement | null>(
  () => layoutRef.value?.scrollContainerRef ?? null,
)

const navigateToCreateDevice = () => {
  void router.push('/devices/new')
}
const navigateToDevice = (device: Device) => {
  void router.push(`/devices/${device.id}`)
}

// Pre-warm user cache with primary users from the loaded device
// pages so avatar rendering doesn't fan out to N requests.
async function preWarmUserCache(devices: Device[]) {
  try {
    const userUuids = [
      ...new Set(
        devices
          .map((d) => d.primary_user?.uuid)
          .filter((uuid): uuid is string => typeof uuid === 'string' && uuid.length === 36),
      ),
    ]
    if (userUuids.length === 0) return
    const uncached = userUuids.filter((uuid) => !dataStore.getUserName(uuid))
    if (uncached.length === 0) return
    await dataStore.getUsersByUuids(uncached)
  } catch (err) {
    console.warn('Failed to pre-warm user cache:', err)
  }
}

const controls = useListControls<Device>({
  itemIdField: 'id',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  defaultPageSize: 0,
})

const page = useListPage({
  controls,
  keys: devicesKeys,
  fetchPage: (params) => getPaginatedDevices(params, `devices-page-${params.page}`),
  scrollContainerRef,
  sseEvents: ['device-updated', 'device-created', 'device-deleted'],
  mobileSearch: {
    placeholder: 'Search devices...',
    createIcon: 'device',
    onCreate: navigateToCreateDevice,
  },
  urlSync: { paramKeys: ['warranty'] },
  onPageLoaded: preWarmUserCache,
})

usePageCreateAction(navigateToCreateDevice)

const selection = useBulkSelection<Device>({
  items: page.items,
  cacheKey: controls.cacheKeyPart,
  totalCount: page.totalItems,
})
const dt = useBulkSelectionForDataTable(selection)

const filterOptions = computed(() =>
  controls.buildFilterOptions({
    warranty: {
      options: [
        { value: 'active', label: 'Active' },
        { value: 'warning', label: 'Warning' },
        { value: 'expired', label: 'Expired' },
        { value: 'unknown', label: 'Unknown' },
      ],
      width: 'w-[140px]',
      allLabel: 'All Warranties',
    },
  }),
)

// Available sortable fields: id, name, hostname, serial_number,
// model, warranty_status, manufacturer, created_at, updated_at,
// last_sync_time.
const columns = [
  { field: 'name', label: 'Device', width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'serial_number', label: 'Serial', width: 'minmax(140px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'hostname', label: 'Hostname', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'model', label: 'Model', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'primary_user', label: 'User', width: 'minmax(140px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'warranty_status', label: 'Warranty', width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const },
]

const gridClass =
  'grid-cols-[auto_1fr_minmax(100px,auto)] md:grid-cols-[auto_1fr_minmax(140px,auto)_minmax(140px,auto)_minmax(100px,auto)] lg:grid-cols-[auto_1fr_minmax(140px,auto)_minmax(120px,auto)_minmax(120px,auto)_minmax(140px,auto)_minmax(100px,auto)]'

// Bulk delete: irreversible (devices aren't soft-deleted), so a
// confirm modal rather than the optimistic Undo-toast pattern.
const showDeleteConfirm = ref(false)
const bulkDelete = useMutation({
  mutation: (ids: number[]) => bulkAction({ action: 'delete', ids }),
  onSettled: () => queryCache.invalidateQueries({ key: devicesKeys.root }),
  onError: (err) => {
    console.error('Bulk delete failed:', err)
    alert(extractErrorMessage(err, 'Failed to delete devices. Please try again.'))
  },
})

async function confirmDelete() {
  showDeleteConfirm.value = false
  const ids = selection.selectedIds.value.map((id) => parseInt(id))
  if (ids.length === 0) return
  await bulkDelete.mutateAsync(ids)
  selection.clear()
}
</script>

<template>
  <!--
    Single root element. Multi-root (fragment) views break the
    `<Transition mode="out-in">` wrapping our RouterView in
    App.vue: Vue's transition system can't attach
    `.page-leave-active` to a fragment, the leave never finishes,
    and the next route never enters. The dialog teleports to body
    so its DOM placement here is purely organisational; the
    wrapper just gives Transition a real element to bind to.
  -->
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
    search-placeholder="Search devices..."
    item-label="device"
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
        icon="device"
        :title="controls.searchQuery.value ? 'No devices match your search' : 'No devices found'"
        :description="controls.searchQuery.value ? 'Try adjusting your search or filters' : 'Add your first device to get started'"
        :action-label="!controls.searchQuery.value ? 'Add Device' : undefined"
        @action="navigateToCreateDevice"
      />
    </template>

    <template #desktop="{ items, isBackgroundRefresh }">
      <DataTable
        :columns="columns"
        :data="items"
        :selected-items="dt.selectedItems"
        :sort-field="controls.sortField.value"
        :sort-direction="controls.sortDirection.value"
        :grid-class="gridClass"
        :loading="isBackgroundRefresh"
        @update:sort="controls.handleSortUpdate"
        @toggle-selection="dt.onToggleSelection"
        @toggle-all="dt.onToggleAll"
        @row-click="navigateToDevice"
      >
        <template #cell-name="{ item }">
          <div class="flex flex-col gap-0.5">
            <div class="flex items-center gap-1.5">
              <TextCell :value="item.name" font-weight="medium" />
              <div v-if="item.groups?.length" class="flex items-center gap-1 flex-shrink-0">
                <span
                  v-for="group in item.groups.slice(0, 3)"
                  :key="group.id"
                  class="w-2 h-2 rounded-full flex-shrink-0"
                  :style="{ backgroundColor: group.color || 'var(--color-text-tertiary)' }"
                  :title="group.name"
                />
                <span v-if="item.groups.length > 3" class="text-[10px] text-tertiary">+{{ item.groups.length - 3 }}</span>
              </div>
            </div>
            <span v-if="item.manufacturer" class="text-xs text-tertiary">{{ item.manufacturer }}</span>
          </div>
        </template>

        <template #cell-serial_number="{ item }">
          <span class="text-xs font-mono text-secondary">{{ item.serial_number || '—' }}</span>
        </template>

        <template #cell-hostname="{ item }">
          <span class="text-xs font-mono text-secondary truncate">{{ item.hostname || '—' }}</span>
        </template>

        <template #cell-model="{ item }">
          <TextCell :value="item.model || '—'" />
        </template>

        <template #cell-primary_user="{ item }">
          <UserAvatarCell
            v-if="item.primary_user"
            :user-id="item.primary_user.uuid"
            :user-name="item.primary_user.name"
            :avatar="item.primary_user.avatar_thumb || item.primary_user.avatar_url"
            :show-name="true"
          />
          <span v-else class="text-xs text-tertiary">Unassigned</span>
        </template>

        <template #cell-warranty_status="{ item }">
          <StatusBadgeCell type="warranty" :value="item.warranty_status" />
        </template>
      </DataTable>
    </template>

    <template #mobile-row="{ item }">
      <div
        class="flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer border-t border-default first:border-t-0"
        @click="navigateToDevice(item)"
      >
        <div class="w-10 h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
          <svg class="w-5 h-5 text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
          </svg>
        </div>

        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-1.5">
            <span class="text-sm text-primary font-medium truncate">{{ item.name }}</span>
            <div v-if="item.groups?.length" class="flex items-center gap-1 flex-shrink-0">
              <span
                v-for="group in item.groups.slice(0, 3)"
                :key="group.id"
                class="w-1.5 h-1.5 rounded-full"
                :style="{ backgroundColor: group.color || 'var(--color-text-tertiary)' }"
                :title="group.name"
              />
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-1 text-xs">
            <span v-if="item.model" class="text-secondary">{{ item.model }}</span>
            <span v-if="item.serial_number" class="text-tertiary font-mono">{{ item.serial_number }}</span>
          </div>

          <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-0.5 text-xs">
            <span v-if="item.hostname" class="text-tertiary font-mono truncate max-w-[160px]">{{ item.hostname }}</span>
            <span v-if="item.primary_user" class="text-secondary truncate max-w-[120px]">{{ item.primary_user.name }}</span>
            <span
              class="inline-flex items-center px-1.5 py-0.5 rounded font-medium border"
              :class="{
                'bg-status-success-muted text-status-success border-status-success/30': item.warranty_status === 'Active',
                'bg-status-warning-muted text-status-warning border-status-warning/30': item.warranty_status === 'Warning',
                'bg-status-error-muted text-status-error border-status-error/30': item.warranty_status === 'Expired',
                'bg-surface-alt text-secondary border-default': !item.warranty_status || item.warranty_status === 'Unknown'
              }"
            >
              {{ item.warranty_status || 'Unknown' }}
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
        class="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-full text-status-error hover:bg-status-error/10 transition-colors whitespace-nowrap disabled:opacity-50"
        :disabled="bulkDelete.asyncStatus.value === 'loading'"
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
        :total-items="page.totalItems.value"
        :page-size="controls.pageSize.value"
        :page-size-options="controls.pageSizeOptions"
        :is-infinite-mode="controls.isInfiniteMode.value"
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
    item-label="device"
    action-verb="delete"
    @confirm="confirmDelete"
    @close="showDeleteConfirm = false"
  />
  </div>
</template>
