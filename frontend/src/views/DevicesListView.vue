<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import BaseListView from '@/components/common/BaseListView.vue'
import DataTable from '@/components/common/DataTable.vue'
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue'
import PaginationControls from '@/components/common/PaginationControls.vue'

import { TextCell, StatusBadgeCell, UserAvatarCell } from '@/components/common/cells'
import { useListManagement } from '@/composables/useListManagement'
import { useListSSE } from '@/composables/useListSSE'
import { useStaggeredList } from '@/composables/useStaggeredList'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { useDataStore } from '@/stores/dataStore'
import { getPaginatedDevices } from '@/services/deviceService'
import type { Device } from '@/types/device'

const router = useRouter()
const dataStore = useDataStore()

// Mobile detection for conditional infinite scroll
const { isMobile } = useMobileDetection()

// Default page size: 0 (infinite scroll) on mobile, 25 on desktop
const defaultPageSize = isMobile.value ? 0 : 25

// Navigate to create device (used by both header button and mobile search bar)
const navigateToCreateDevice = () => {
  router.push('/devices/new');
};

// Use the composable for all common functionality
const listManager = useListManagement<Device>({
  defaultPageSize,
  itemIdField: 'id',
  defaultSortField: 'name',
  defaultSortDirection: 'asc',
  fetchFunction: async (params) => {
    // Use unique request key per page to prevent cancellation during infinite scroll
    const requestKey = `paginated-devices-page-${params.page}`;
    const response = await getPaginatedDevices({
      page: params.page,
      pageSize: params.pageSize,
      sortField: params.sortField,
      sortDirection: params.sortDirection,
      search: params.search,
      type: params.type,
      warranty: params.warranty
    }, requestKey);

    // Pre-warm user cache for efficient avatar loading
    preWarmUserCache(response.data);

    return response;
  },
  routeBuilder: (device) => `/devices/${device.id}`,
  mobileSearch: {
    placeholder: 'Search devices...',
    createIcon: 'device',
    onCreate: navigateToCreateDevice
  }
});

// SSE integration for real-time updates
useListSSE<Device>({
  hasItem: listManager.hasItem,
  updateItemField: listManager.updateItemField,
  removeItem: listManager.removeItem,
  prependItem: listManager.prependItem,
  eventTypes: { updated: 'device-updated' },
  getEventItemId: (data) => (data.data || data).device_id
});

// Pre-warm user cache with all primary users from devices
const preWarmUserCache = async (devices: Device[]) => {
  try {
    // Extract unique user UUIDs from devices
    const userUuids = [...new Set(
      devices
        .map(device => device.primary_user?.uuid)
        .filter((uuid): uuid is string => uuid !== undefined && uuid.length === 36) // Valid UUIDs only
    )];
    
    if (userUuids.length === 0) return;
    
    console.log(`🚀 Pre-warming user cache for ${userUuids.length} device users`);
    
    // Check which users are already cached to avoid unnecessary requests
    const uncachedUuids = userUuids.filter(uuid => {
      const cachedName = dataStore.getUserName(uuid);
      return !cachedName; // If no cached name, user is not cached
    });
    
    if (uncachedUuids.length === 0) {
      console.log('✅ All device users already cached, skipping pre-warm');
      return;
    }
    
    console.log(`📡 Loading ${uncachedUuids.length} uncached users (${userUuids.length - uncachedUuids.length} already cached)`);
    
    // Use our batching system to efficiently load only uncached users
    await dataStore.getUsersByUuids(uncachedUuids);
    
    console.log('✅ Device user cache pre-warming completed');
  } catch (error) {
    console.warn('Failed to pre-warm user cache:', error);
    // Don't throw - this is an optimization, not critical
  }
};

// Define table columns with responsive behavior
// Available sortable fields: id, name, hostname, serial_number, model, warranty_status, manufacturer, created_at, updated_at, last_sync_time
const columns = [
  { field: 'name', label: 'Device', width: '1fr', sortable: true, responsive: 'always' as const },
  { field: 'serial_number', label: 'Serial', width: 'minmax(140px,auto)', sortable: true, responsive: 'md' as const },
  { field: 'model', label: 'Model', width: 'minmax(140px,auto)', sortable: true, responsive: 'lg' as const },
  { field: 'primary_user', label: 'User', width: 'minmax(140px,auto)', sortable: false, responsive: 'md' as const },
  { field: 'warranty_status', label: 'Warranty', width: 'minmax(100px,auto)', sortable: true, responsive: 'always' as const }
];

// Build filter options - warranty is the reliable filter from the API
const filterOptions = listManager.buildFilterOptions({
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
});

// Custom grid template for responsive layout (includes checkbox column with auto width)
const gridClass = "grid-cols-[auto_1fr_minmax(100px,auto)] md:grid-cols-[auto_1fr_minmax(140px,auto)_minmax(140px,auto)_minmax(100px,auto)] lg:grid-cols-[auto_1fr_minmax(140px,auto)_minmax(140px,auto)_minmax(140px,auto)_minmax(100px,auto)]";

// Staggered fade-in animation
const { getStyle } = useStaggeredList();

// Track if we're currently loading more (to prevent duplicate requests)
const isLoadingMore = ref(false);

// Handle load more from BaseListView's scroll event
const handleLoadMore = async () => {
  if (isLoadingMore.value) return;
  isLoadingMore.value = true;
  try {
    await listManager.loadMore();
  } finally {
    isLoadingMore.value = false;
  }
};

// Expose method for parent (App.vue) to call from header button
defineExpose({
  navigateToCreateDevice
});
</script>

<template>
  <div class="flex flex-col h-full overflow-hidden">
    <!-- Search and filter bar -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2 flex-wrap">
        <!-- Search input - hidden on mobile (shown in MobileSearchBar) -->
        <DebouncedSearchInput
          :model-value="listManager.searchQuery.value"
          @update:model-value="listManager.handleSearchUpdate"
          placeholder="Search devices..."
          class="hidden sm:block"
        />

        <!-- Filters -->
        <template v-if="filterOptions.length > 0">
          <div 
            v-for="filter in filterOptions" 
            :key="filter.name"
            :class="[filter.width || 'w-[120px]']"
          >
            <select
              :value="filter.value"
              @change="e => listManager.handleFilterUpdate(filter.name, (e.target as HTMLSelectElement).value)"
              class="bg-surface-alt border border-default text-primary text-sm rounded-md focus:ring-accent focus:border-accent block w-full py-1 px-2"
            >
              <option
                v-for="option in filter.options"
                :key="option.value"
                :value="option.value"
              >
                {{ option.label }}
              </option>
            </select>
          </div>

          <button
            @click="listManager.resetFilters"
            class="px-2 py-1 text-xs font-medium text-white bg-accent rounded-md hover:opacity-90 focus:ring-2 focus:outline-none focus:ring-accent"
          >
            Reset
          </button>
        </template>

        <div class="text-xs text-secondary ml-auto">
          {{ listManager.totalItems.value }} result{{ listManager.totalItems.value !== 1 ? "s" : "" }}
        </div>
      </div>
    </div>

    <!-- Main content -->
    <div class="flex-1 flex flex-col overflow-hidden">
      <BaseListView
        title="Devices"
        :is-loading="listManager.loading.value"
        :is-empty="listManager.items.value.length === 0 && !listManager.loading.value"
        :error="listManager.error.value"
        :is-mobile="isMobile"
        empty-icon="device"
        :empty-message="listManager.searchQuery.value ? 'No devices match your search' : 'No devices found'"
        :empty-description="listManager.searchQuery.value ? 'Try adjusting your search or filters' : 'Add your first device to get started'"
        :empty-action-label="!listManager.searchQuery.value ? 'Add Device' : undefined"
        :is-loading-more="isLoadingMore"
        @retry="listManager.fetchItems"
        @empty-action="navigateToCreateDevice"
        @load-more="handleLoadMore"
      >
        <!-- Desktop Table View -->
        <template #default>
          <div class="flex-1 overflow-y-auto">
            <DataTable
              :columns="columns"
              :data="listManager.items.value"
              :selected-items="listManager.selectedItems.value"
              :sort-field="listManager.sortField.value"
              :sort-direction="listManager.sortDirection.value"
              :grid-class="gridClass"
              @update:sort="listManager.handleSortUpdate"
              @toggle-selection="listManager.toggleSelection"
              @toggle-all="listManager.toggleAllItems"
              @row-click="listManager.navigateToItem"
            >
            <!-- Custom cell templates -->
            <template #cell-name="{ item }">
              <div class="flex flex-col">
                <TextCell :value="item.name" font-weight="medium" />
                <span v-if="item.manufacturer" class="text-xs text-tertiary">{{ item.manufacturer }}</span>
              </div>
            </template>

            <template #cell-serial_number="{ value }">
              <span class="text-xs font-mono text-secondary">{{ value }}</span>
            </template>

            <template #cell-model="{ value }">
              <TextCell :value="value || 'Unknown'" />
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
        </template>

        <!-- Mobile/Tablet Card View -->
        <template #mobile-view>
          <div class="flex-1 overflow-y-auto">
            <TransitionGroup
              name="list-stagger"
              tag="div"
              class="flex flex-col"
            >
            <div
              v-for="(device, index) in listManager.items.value"
              :key="device.id"
              :style="getStyle(index)"
              @click="listManager.navigateToItem(device)"
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
                <!-- Name -->
                <div class="text-sm text-primary font-medium truncate">{{ device.name }}</div>

                <!-- Meta row -->
                <div class="flex flex-wrap items-center gap-2 mt-1 text-xs">
                  <!-- Model -->
                  <span class="text-secondary">{{ device.model || 'Unknown model' }}</span>

                  <!-- Serial -->
                  <span class="text-tertiary font-mono">{{ device.serial_number }}</span>

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

                  <!-- Primary User -->
                  <template v-if="device.primary_user">
                    <span class="text-secondary truncate max-w-[120px]">{{ device.primary_user.name }}</span>
                  </template>
                </div>
              </div>

              <!-- Chevron -->
              <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
              </svg>
            </div>
            </TransitionGroup>
          </div>
        </template>
      </BaseListView>
    </div>

    <!-- Pagination Controls (hidden on mobile when using infinite scroll) -->
    <PaginationControls
      v-if="!isMobile"
      :current-page="listManager.currentPage.value"
      :total-pages="listManager.totalPages.value"
      :page-size="listManager.pageSize.value"
      :page-size-options="listManager.pageSizeOptions"
      :show-import="true"
      @update:current-page="listManager.handlePageChange"
      @update:page-size="listManager.handlePageSizeChange"
      @import="() => {}"
    />

  </div>
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