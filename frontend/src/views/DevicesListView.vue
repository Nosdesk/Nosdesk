<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useInfiniteQuery, useMutation, useQueryCache } from '@pinia/colada'
import { useSSE } from '@/services/sseService'
import PageScroll from '@/components/common/PageScroll.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import DataTable from '@/components/common/DataTable.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'
import BulkActionsBar from '@/components/common/BulkActionsBar.vue'
import type { BulkAction } from '@/components/common/BulkActionsBar.vue'

import { TextCell, StatusBadgeCell, UserAvatarCell } from '@/components/common/cells'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import { useListControls } from '@/composables/useListControls'
import { useMobileSearch } from '@/composables/useMobileSearch'
import { useStaggeredList } from '@/composables/useStaggeredList'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { useInfiniteScroll } from '@/composables/useInfiniteScroll'
import { useDataStore } from '@/stores/dataStore'
import { getPaginatedDevices, bulkAction } from '@/services/deviceService'
import type { Device } from '@/types/device'

const DEVICES_KEYS = {
  root: ['devices'] as const,
  list: (variant: 'infinite' | 'paginated', cacheKey: string, page?: number) =>
    page === undefined
      ? ([...DEVICES_KEYS.root, 'list', variant, cacheKey] as const)
      : ([...DEVICES_KEYS.root, 'list', variant, cacheKey, page] as const),
}

defineOptions({ name: 'DevicesListView' })

const router = useRouter()
const dataStore = useDataStore()
const queryCache = useQueryCache()

const { isMobile } = useMobileDetection()

const pageScrollRef = ref<InstanceType<typeof PageScroll> | null>(null)
const scrollContainerRef = computed<HTMLElement | null>(
  () => pageScrollRef.value?.scrollContainerRef ?? null,
)

const navigateToCreateDevice = () => {
  router.push('/devices/new')
}

const navigateToDevice = (device: Device) => {
  router.push(`/devices/${device.id}`)
}

// Pre-warm user cache with all primary users from devices.
// Side effect of fetching device pages: needed for avatar
// rendering without N requests per device row.
const preWarmUserCache = async (devices: Device[]) => {
  try {
    const userUuids = [...new Set(
      devices
        .map(device => device.primary_user?.uuid)
        .filter((uuid): uuid is string => uuid !== undefined && uuid.length === 36)
    )]
    if (userUuids.length === 0) return
    const uncachedUuids = userUuids.filter(uuid => !dataStore.getUserName(uuid))
    if (uncachedUuids.length === 0) return
    await dataStore.getUsersByUuids(uncachedUuids)
  } catch (error) {
    console.warn('Failed to pre-warm user cache:', error)
  }
}

// UI-state composable.
const controls = useListControls<Device>({
  itemIdField: 'id',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  defaultPageSize: 0,
})

// Pinia Colada query layer (dual-mode infinite + paged).
const infiniteList = useInfiniteQuery(() => ({
  key: DEVICES_KEYS.list('infinite', controls.cacheKeyPart.value),
  initialPageParam: 1,
  query: async ({ pageParam }) => {
    const response = await getPaginatedDevices(
      { ...controls.requestParams.value, page: pageParam },
      `devices-infinite-page-${pageParam}`,
    )
    preWarmUserCache(response.data)
    return response
  },
  getNextPageParam: (lastPage, allPages) =>
    allPages.length < lastPage.totalPages ? allPages.length + 1 : null,
  enabled: () => controls.isInfiniteMode.value,
}))

const paginatedList = useInfiniteQuery(() => ({
  key: DEVICES_KEYS.list('paginated', controls.cacheKeyPart.value, controls.currentPage.value),
  initialPageParam: controls.currentPage.value,
  query: async ({ pageParam }) => {
    const response = await getPaginatedDevices(
      { ...controls.requestParams.value, page: pageParam },
      `devices-paginated-page-${pageParam}`,
    )
    preWarmUserCache(response.data)
    return response
  },
  getNextPageParam: () => null,
  enabled: () => !controls.isInfiniteMode.value,
}))

const items = computed<Device[]>(() => {
  const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
  return source.data.value?.pages.flatMap(p => p.data) ?? []
})

const totalItems = computed(() => {
  const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
  return source.data.value?.pages[0]?.total ?? 0
})

const totalPages = computed(() => {
  const source = controls.isInfiniteMode.value ? infiniteList : paginatedList
  return source.data.value?.pages[0]?.totalPages ?? 1
})

const hasMore = computed(() =>
  controls.isInfiniteMode.value ? infiniteList.hasNextPage.value : false,
)

const activeAsyncStatus = computed(() =>
  controls.isInfiniteMode.value
    ? infiniteList.asyncStatus.value
    : paginatedList.asyncStatus.value,
)

const activeError = computed(() =>
  controls.isInfiniteMode.value ? infiniteList.error.value : paginatedList.error.value,
)

const isFetching = computed(() => activeAsyncStatus.value === 'loading')
const isFirstLoad = computed(() => isFetching.value && items.value.length === 0)
const isLoadingMore = computed(() => isFetching.value && items.value.length > 0)
const isBackgroundRefresh = computed(() => isFetching.value && items.value.length > 0)

// SSE-driven cache invalidation.
const sse = useSSE()
function invalidateDevicesList() {
  queryCache.invalidateQueries({ key: DEVICES_KEYS.root })
}

const sseHandlers: Array<{ type: string; handler: (data: unknown) => void }> = [
  { type: 'device-updated', handler: invalidateDevicesList },
  { type: 'device-created', handler: invalidateDevicesList },
  { type: 'device-deleted', handler: invalidateDevicesList },
]

onMounted(() => {
  if (!sse.isConnected.value) sse.connect()
  for (const { type, handler } of sseHandlers) {
    sse.addEventListener(type as never, handler)
  }
})
onUnmounted(() => {
  for (const { type, handler } of sseHandlers) {
    sse.removeEventListener(type as never, handler)
  }
})

// Mobile search bar wiring.
const mobileSearch = useMobileSearch()
function setupMobileSearch() {
  mobileSearch.registerMobileSearch({
    searchQuery: controls.searchQuery.value,
    placeholder: 'Search devices...',
    showCreateButton: true,
    createIcon: 'device',
    onSearchUpdate: controls.handleSearchUpdate,
    onCreate: navigateToCreateDevice,
  })
}
onMounted(setupMobileSearch)
onUnmounted(mobileSearch.deregisterMobileSearch)
watch(controls.searchQuery, mobileSearch.updateSearchQuery)

// Infinite scroll uses PageScroll's single scroll container.
useInfiniteScroll({
  containerRef: scrollContainerRef,
  enabled: controls.isInfiniteMode,
  hasMore,
  isLoading: computed(() => isLoadingMore.value),
  onLoadMore: () => infiniteList.loadNextPage(),
})

// Define table columns with responsive behavior
// Available sortable fields: id, name, hostname, serial_number, model, warranty_status, manufacturer, created_at, updated_at, last_sync_time
const columns = [
  { field: 'name', label: 'Device', width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'serial_number', label: 'Serial', width: 'minmax(140px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'hostname', label: 'Hostname', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'model', label: 'Model', width: 'minmax(120px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'primary_user', label: 'User', width: 'minmax(140px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'warranty_status', label: 'Warranty', width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const }
];

// Build filter options - warranty is the reliable filter from the API
const filterOptions = computed(() => controls.buildFilterOptions({
  warranty: {
    options: [
      { value: 'active', label: 'Active' },
      { value: 'warning', label: 'Warning' },
      { value: 'expired', label: 'Expired' },
      { value: 'unknown', label: 'Unknown' }
    ],
    width: 'w-[140px]',
    allLabel: 'All Warranties'
  }
}));

// Custom grid template for responsive layout (includes checkbox column with auto width)
const gridClass = "grid-cols-[auto_1fr_minmax(100px,auto)] md:grid-cols-[auto_1fr_minmax(140px,auto)_minmax(140px,auto)_minmax(100px,auto)] lg:grid-cols-[auto_1fr_minmax(140px,auto)_minmax(120px,auto)_minmax(120px,auto)_minmax(140px,auto)_minmax(100px,auto)]";

// Staggered fade-in animation
const { getStyle } = useStaggeredList();

// Bulk actions configuration
const bulkActions: BulkAction[] = [
  { id: 'delete', label: 'Delete', icon: 'delete', variant: 'danger', confirm: true }
];

const bulkActionMutation = useMutation({
  mutation: (vars: { action: 'delete'; ids: number[] }) => bulkAction(vars),
  onSettled: () => {
    queryCache.invalidateQueries({ key: DEVICES_KEYS.root })
  },
  onError: (err) => {
    console.error('Bulk action failed:', err)
    alert('Failed to perform bulk action. Please try again.')
  },
})
const bulkActionLoading = computed(() => bulkActionMutation.asyncStatus.value === 'loading')

const handleBulkAction = async (actionId: string) => {
  if (actionId === 'delete') {
    await executeBulkAction('delete')
  }
}

const executeBulkAction = async (action: 'delete') => {
  const ids = controls.selectedItems.value.map(id => parseInt(id))
  if (ids.length === 0) return
  await bulkActionMutation.mutateAsync({ action, ids })
  controls.clearSelection()
}

// Selection / retry handlers that need access to `items`.
const handleToggleSelection = (event: Event, itemId: string) =>
  controls.toggleSelection(event, itemId, items.value)
const handleToggleAll = (event: Event) =>
  controls.toggleAllItems(event, items.value)
const handleSelectAll = () => controls.selectAll(items.value)
const handleRetry = () => {
  if (controls.isInfiniteMode.value) infiniteList.refetch()
  else paginatedList.refetch()
}
const errorMessage = computed(() => {
  if (!activeError.value) return null
  return activeError.value instanceof Error
    ? activeError.value.message
    : 'Failed to load devices. Please try again.'
})

// Expose method for parent (App.vue) to call from header button
defineExpose({
  navigateToCreateDevice
});
</script>

<template>
  <PageScroll
    ref="pageScrollRef"
    content-class="flex h-full flex-col"
    :is-empty="!!errorMessage || (items.length === 0 && !isFirstLoad)"
  >
    <template #chrome>
      <!-- Search and filter bar -->
      <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
        <div class="p-2 flex items-center gap-2 flex-wrap">
          <DebouncedSearchInput
            :model-value="controls.searchQuery.value"
            @update:model-value="controls.handleSearchUpdate"
            placeholder="Search devices..."
            class="hidden sm:block"
          />

          <template v-if="filterOptions.length > 0">
            <div
              v-for="filter in filterOptions"
              :key="filter.name"
              :class="[filter.width || 'w-[120px]']"
            >
              <BaseDropdown
                :model-value="filter.value"
                :options="filter.options"
                :multiple="filter.multiple"
                :placeholder="filter.placeholder"
                size="sm"
                @update:model-value="value => controls.handleFilterUpdate(filter.name, value)"
              />
            </div>

            <button
              @click="controls.resetFilters"
              class="px-2 py-1 text-xs font-medium text-white bg-accent rounded-md hover:opacity-90 focus:ring-2 focus:outline-none focus:ring-accent"
            >
              Reset
            </button>
          </template>

          <div class="text-xs text-secondary ml-auto">
            {{ totalItems }} result{{ totalItems !== 1 ? "s" : "" }}
          </div>
        </div>
      </div>

      <BulkActionsBar
        :selected-count="controls.selectedItems.value.length"
        :total-count="totalItems"
        :actions="bulkActions"
        item-label="device"
        @action="handleBulkAction"
        @clear-selection="controls.clearSelection"
        @select-all="handleSelectAll"
      />
    </template>

    <template #empty>
      <ErrorBanner
        v-if="errorMessage"
        :message="errorMessage"
        :show-retry="true"
        @retry="handleRetry"
      />
      <EmptyState
        v-else
        icon="device"
        :title="controls.searchQuery.value ? 'No devices match your search' : 'No devices found'"
        :description="controls.searchQuery.value ? 'Try adjusting your search or filters' : 'Add your first device to get started'"
        :action-label="!controls.searchQuery.value ? 'Add Device' : undefined"
        @action="navigateToCreateDevice"
      />
    </template>

    <div v-show="!isMobile" class="flex h-full flex-col">
      <DataTable
              :columns="columns"
              :data="items"
              :selected-items="controls.selectedItems.value"
              :sort-field="controls.sortField.value"
              :sort-direction="controls.sortDirection.value"
              :grid-class="gridClass"
              @update:sort="controls.handleSortUpdate"
              @toggle-selection="handleToggleSelection"
              @toggle-all="handleToggleAll"
              @row-click="navigateToDevice"
            >
            <!-- Custom cell templates -->
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

            <template #cell-serial_number="{ value }">
              <span class="text-xs font-mono text-secondary">{{ value || '—' }}</span>
            </template>

            <template #cell-hostname="{ value }">
              <span class="text-xs font-mono text-secondary truncate">{{ value || '—' }}</span>
            </template>

            <template #cell-model="{ value }">
              <TextCell :value="value || '—'" />
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

            <template #cell-warranty_status="{ value }">
              <StatusBadgeCell type="warranty" :value="value" />
            </template>
      </DataTable>
    </div>

    <div v-show="isMobile" class="flex h-full flex-col">
      <TransitionGroup
        name="list-stagger"
        tag="div"
        class="flex flex-col"
      >
            <div
              v-for="(device, index) in items"
              :key="device.id"
              :style="getStyle(index)"
              @click="navigateToDevice(device)"
              :class="[
                'flex items-center gap-3 px-3 py-2.5 hover:bg-surface-hover active:bg-surface-alt transition-colors cursor-pointer',
                index > 0 ? 'border-t border-default' : ''
              ]"
            >
              <!-- Device icon -->
              <div class="w-10 h-10 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
                <svg class="w-5 h-5 text-secondary" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                </svg>
              </div>

              <!-- Main content -->
              <div class="flex-1 min-w-0">
                <!-- Name + groups -->
                <div class="flex items-center gap-1.5">
                  <span class="text-sm text-primary font-medium truncate">{{ device.name }}</span>
                  <div v-if="device.groups?.length" class="flex items-center gap-1 flex-shrink-0">
                    <span
                      v-for="group in device.groups.slice(0, 3)"
                      :key="group.id"
                      class="w-1.5 h-1.5 rounded-full"
                      :style="{ backgroundColor: group.color || 'var(--color-text-tertiary)' }"
                      :title="group.name"
                    />
                  </div>
                </div>

                <!-- Meta row 1: model + serial -->
                <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-1 text-xs">
                  <span v-if="device.model" class="text-secondary">{{ device.model }}</span>
                  <span v-if="device.serial_number" class="text-tertiary font-mono">{{ device.serial_number }}</span>
                </div>

                <!-- Meta row 2: hostname, user, warranty -->
                <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-0.5 text-xs">
                  <span v-if="device.hostname" class="text-tertiary font-mono truncate max-w-[160px]">{{ device.hostname }}</span>

                  <span v-if="device.primary_user" class="text-secondary truncate max-w-[120px]">{{ device.primary_user.name }}</span>

                  <!-- Warranty Status -->
                  <span
                    class="inline-flex items-center px-1.5 py-0.5 rounded font-medium border"
                    :class="{
                      'bg-status-success-muted text-status-success border-status-success/30': device.warranty_status === 'Active',
                      'bg-status-warning-muted text-status-warning border-status-warning/30': device.warranty_status === 'Warning',
                      'bg-status-error-muted text-status-error border-status-error/30': device.warranty_status === 'Expired',
                      'bg-surface-alt text-secondary border-default': !device.warranty_status || device.warranty_status === 'Unknown'
                    }"
                  >
                    {{ device.warranty_status || 'Unknown' }}
                  </span>
                </div>
              </div>

              <!-- Chevron -->
              <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
              </svg>
      </div>
      </TransitionGroup>
    </div>

    <template #footer>
      <PaginationControls
        v-if="!isMobile"
        :current-page="controls.currentPage.value"
        :total-pages="totalPages"
        :page-size="controls.pageSize.value"
        :page-size-options="controls.pageSizeOptions"
        :show-import="true"
        @update:current-page="controls.handlePageChange"
        @update:page-size="controls.handlePageSizeChange"
        @import="() => {}"
      />
    </template>
  </PageScroll>
</template>

<style scoped>
/* Custom scrollbar styling */
.overflow-y-auto::-webkit-scrollbar,
.overflow-x-auto::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

.overflow-y-auto::-webkit-scrollbar-track,
.overflow-x-auto::-webkit-scrollbar-track {
  background: var(--color-bg-surface);
}

.overflow-y-auto::-webkit-scrollbar-thumb,
.overflow-x-auto::-webkit-scrollbar-thumb {
  background: var(--color-border-default);
  border-radius: 4px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover,
.overflow-x-auto::-webkit-scrollbar-thumb:hover {
  background: var(--color-border-strong);
}

.overflow-x-auto::-webkit-scrollbar-corner {
  background: var(--color-bg-surface);
}
</style> 